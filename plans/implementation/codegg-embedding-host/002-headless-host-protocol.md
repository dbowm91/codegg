# CodeGG Embedding Host Milestone 002 — Headless host protocol v1

Status: proposed

Repository baseline: `1d2d9dce2`

Source roadmap:

- `plans/subsystems/codegg-embedding-host-roadmap.md#7-milestones` (E002)

Long-term requirements:

- `plans/000-long-term-specification.md` — host-neutral execution, secret handling
- `plans/001-terminology-and-domain-model.md` — session, turn, provider connection

Applicable ADRs:

- ADR-0061 (accepted) — the embedding wire is a public contract with its own
  stability policy. This plan implements that decision.

Primary class: capability

## 1. Objective

Add `codegg host --stdio`: one process, one workspace, one session, driven by a
versioned newline-delimited JSON-RPC 2.0 protocol over stdin/stdout. A host can
open a session with an inline provider descriptor, submit a turn, receive
structured events and usage, cancel the turn, and close the session — with no
daemon, no global config and no project or git requirement.

## 2. Why this milestone is ready

- E001 provides the inline descriptor and the header-carrying provider.
- `src/exec.rs` already drives the real loop headlessly from a JSON input and
  builds its registry from config; the host mode reuses that setup with the
  descriptor path instead.
- The loop already emits the events the wire needs through its existing bus and
  turn completion; `src/acp.rs` proves a stdio adapter can sit on top of it.
- The helper-pipe transport is already how a VM host reaches a guest process.

Hard dependency: E001.

## 3. Current implementation evidence

- `src/acp.rs` is a stdio JSON-RPC adapter with a fixed, minimal method set and
  no `_meta` handling; it is the proof that stdio adapters are viable but not a
  suitable base for provider descriptors.
- `ExecMode` (`src/exec.rs`) owns the loop setup: config load, registry
  registration, provider resolution, agent resolution, policy application, then
  `run`. It has no streaming, no cancellation and no workspace field.
- The agent loop (`src/agent/loop.rs`) already takes approval mode and sandbox
  profile per instance.
- `CoreResponse::SessionLaunch` and the daemon protocol assume projects and
  worktrees; the host protocol must not.

## 4. Invariants that must not regress

- ACP v1 and `codegg exec` are unchanged, including their JSON shapes.
- No daemon, no global config, and no TUI state is required for a host session.
- stdout carries only protocol frames; every diagnostic goes to stderr and is
  redacted.
- One active turn per session; a second submit is a typed busy refusal.
- Cancellation is bounded and leaves the process able to accept the next turn.
- The workspace root is canonicalized, must exist, and is the only writable
  root the turn is given.

## 5. Scope

### In scope

- `Commands::Host { stdio: bool }` (v1: `--stdio` only) in `src/main.rs`.
- A new `src/host/` module: wire types, frame reader/writer, session state, turn
  lifecycle, event mapping, cancellation.
- Wire `apiVersion: 1` and an `initialize` capability list.
- Methods: `initialize`, `session/open`, `turn/submit`, `turn/cancel`,
  `session/close`.
- Notifications: `turn/text`, `turn/tool`, `turn/usage`, `turn/done`,
  `turn/error`.
- Structured error data: `{class, retryable, providerStatus?}`.
- Workspace-agnostic session open, descriptor pass-through, approval and sandbox
  per session.

### Explicitly out of scope

- Concurrent turns in one process.
- Attachments and image prompts.
- Tool allow-lists, network policy beyond the existing sandbox profiles.
- Remote/network transport; only stdio.
- Session persistence or resume across process restarts.
- The SDK crate and its client (E003).

## 6. Required production changes

### Core/domain

- `src/host/protocol.rs`: request/notification/error types, `API_VERSION`,
  bounds for every field, and a version check.
- `src/host/session.rs`: one session — workspace root, model, agent, approval,
  sandbox, descriptor, optional stored session id, active-turn state.
- `src/host/turn.rs`: submit, event mapping, cancel, completion.

### Storage and migrations

None. Nothing is persisted.

### Protocol and DTOs

New types live in the binary crate for v1. If E003 extracts a
`codegg-host-protocol` crate, these move there without a wire change; the plan
records that move as an allowed refactor in E003.

### Runtime and concurrency

- Refactor the loop setup currently inside `ExecMode::run` into a shared
  `HeadlessRuntime` used by both `exec` and `host`, so the two cannot drift.
- One active turn guarded by a session mutex. `turn.cancel` sets the loop's
  cancellation token; the turn ends with `stopReason: "cancelled"` within the
  bound. A host that closes stdin cancels and exits cleanly.

### Frontend or operator surface

None. `host` is a new subcommand with no TUI surface.

### Security and authorization

- The descriptor arrives in `session/open` over stdin; a secret is never on argv.
- Frame logging is off by default; a debug mode prints method names and ids only.
- Invalid frames are rejected with a typed error and do not kill the process
  unless the stream is unparseable (then exit with a stable code).

### Documentation and static guards

- `docs/embedding.md` skeleton (the full contract is E003's deliverable).
- A guard that `src/host/` never reads `Config::load()` on the descriptor path
  and never writes a file under the user's config root.

## 7. Ordered work packages

### Work package A — Wire types and versioning

Intent: a frozen, testable wire.

Required changes: request/notification/error enums with `serde` tags, bounded
string/array fields, `apiVersion`, `initialize` response with capabilities and
`codeggVersion`.

Acceptance evidence: a serialize/deserialize table for every message; an
`initialize` golden frame; a version-mismatch refusal.

### Work package B — Headless runtime extraction

Intent: `exec` and `host` share one loop setup.

Required changes: move the provider/agent/policy setup out of `ExecMode` into a
`HeadlessRuntime` that accepts either config or a descriptor; `exec` keeps its
current behaviour byte for byte.

Acceptance evidence: the existing `exec` suite passes unchanged; a new test
builds the runtime from a descriptor.

### Work package C — Session and turn lifecycle

Intent: the protocol does what it says.

Required changes: `session/open` validation (workspace exists and is a
directory, model non-empty, agent resolvable, policy bounds), one active turn,
`turn/submit` returning a `turnId`, `session/close`.

Acceptance evidence: a scripted host opens, submits and closes against a fake
provider; a second submit while busy returns the busy class; an invalid
workspace is refused without side effects.

### Work package D — Streaming events and usage

Intent: a host can show progress and account for cost.

Required changes: map loop events to `turn/text`, `turn/tool`, `turn/usage`;
emit `turn/done {stopReason, usage, inputTokens, outputTokens, cachedTokens?}`;
mark usage explicitly absent when the provider reports none.

Acceptance evidence: a fake provider streams three text chunks and one tool
call; the host observes ordered notifications and a final `turn/done`; absent
usage is `{reported: false}` rather than zero.

### Work package E — Cancellation, errors and shutdown

Intent: nothing hangs, nothing leaks.

Required changes: `turn/cancel` sets the token, the loop stops, `turn/done`
reports `cancelled`; provider failures become `turn/error` with the E001 class;
stdin EOF cancels and exits 0; an unparseable frame exits non-zero with a stable
code.

Acceptance evidence: cancel during a slow provider call ends within the bound
and a subsequent turn succeeds; a 401 produces `auth`; EOF during a turn exits
cleanly.

## 8. Failure, cancellation, restart, and contention semantics

- A provider failure ends the turn, not the process; the session remains open.
- A cancelled turn releases the loop; a second submit is accepted afterwards.
- A host disconnect cancels the active turn and exits; no orphan work continues.
- Duplicate `turn.submit` for the same turn id is refused, not replayed.
- Two processes are independent; no shared state.
- Process restart loses the session by design; the host re-opens.

## 9. Compatibility and migration

Additive. `exec`, ACP and the daemon protocol are untouched. The host protocol
is new and has no legacy path. Within `apiVersion: 1`, new optional fields and
new notifications are allowed; removing or renaming anything requires version 2.

## 10. Required tests

### Focused unit tests

- Frame parse/serialize matrix; oversized frame; invalid UTF-8.
- `session/open` validation matrix.
- Event mapping and usage absence.
- Error class propagation.

### Integration tests

- A scripted host drives a fake provider through open, submit, three events,
  done, close.
- `exec` unchanged.

### Restart and recovery tests

- EOF during a turn exits 0 and leaves no child process.

### Contention and cancellation tests

- Busy refusal; cancel during a slow call; submit after cancel.

### Security and negative tests

- Descriptor secret never appears in stderr, an error frame or a frame dump.
- Unknown method and unknown field handling (unknown method → error, unknown
  field ignored).

### Migration and compatibility tests

- ACP and the existing `exec` JSON shape are unchanged; a golden test pins both.

## 11. Required verification commands

```bash
cargo test -p codegg --bin codegg host
cargo test -p codegg --bin codegg exec
cargo test -p codegg-providers inline_provider
scripts/verify.sh scoped
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Acceptance criteria

1. `codegg host --stdio` answers `initialize` with `apiVersion: 1`, its
   capabilities and its version.
2. `session/open` with a descriptor, workspace root, model, agent and policy
   succeeds without any config file.
3. A turn streams ordered text and tool notifications and ends with
   `turn/done {stopReason, usage}`.
4. `turn/cancel` ends the turn within five seconds and a later turn succeeds.
5. A provider failure is `turn/error` with the documented class and does not kill
   the process.
6. stdin EOF during a turn exits 0 with no orphan child.
7. ACP and `exec` outputs are unchanged.
8. No secret appears on argv, in stderr, in an error frame or in a frame dump.

## 13. Stop conditions

Stop and re-scope if:

- The loop cannot be driven headlessly without the daemon or TUI state.
- Cancellation cannot be bounded without killing the process.
- The shared `HeadlessRuntime` cannot be extracted without changing `exec`.
- A descriptor cannot reach the loop without a global.

## 14. Required closure evidence

Closure record under `plans/closure/codegg-embedding-host/002-status.md`:

- the frozen wire and its golden frames;
- the `exec` regression proof;
- the scripted host transcript (open → events → done → close);
- the cancellation timing and the post-cancel turn;
- the EOF transcript;
- the error-class matrix;
- the redaction proof;
- the accepted ADR reference.
