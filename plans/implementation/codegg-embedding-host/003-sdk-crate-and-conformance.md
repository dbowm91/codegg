# CodeGG Embedding Host Milestone 003 — SDK crate and conformance

Status: proposed

Repository baseline: `1d2d9dce2`

Source roadmap:

- `plans/subsystems/codegg-embedding-host-roadmap.md#7-milestones` (E003)

Long-term requirements:

- `plans/000-long-term-specification.md` — host-neutral execution
- `plans/001-terminology-and-domain-model.md` — session, turn

Applicable ADRs:

- ADR-0060 (accepted) — the embedding wire is a public contract. This
  milestone is where the wire's single definition and its golden frames live.

Primary class: infrastructure

## 1. Objective

Publish the host protocol as a usable library: one crate for the wire types and
one for a typed client, with golden wire tests, a conformance fixture shared with
the host implementation, and `docs/embedding.md` sufficient for a third-party
host to integrate without reading CodeGG internals.

## 2. Why this milestone is ready

- E002 freezes `apiVersion: 1` and proves the wire against a scripted host.
- The wire types currently live in the binary crate; moving them to a crate is a
  mechanical extraction with no wire change.
- `crates/codegg-protocol` + `crates/codegg-client` is the existing precedent for
  splitting types from a client.

Hard dependency: E002.

## 3. Current implementation evidence

- `src/host/protocol.rs` (from E002) holds the v1 types.
- The repository already ships a types crate and a client crate for the
  frontend protocol, and its implementation plans treat golden wire tests as the
  compatibility gate.
- No crate currently exports a documented embedding client.

## 4. Invariants that must not regress

- The wire types are the single definition. The host binary and the SDK MUST
  share them; no second copy is allowed.
- A wire change that is not additive fails the golden tests.
- The client never logs a frame, a header value or a credential.
- The SDK has no dependency on the daemon, the TUI or the frontend protocol.

## 5. Scope

### In scope

- `crates/codegg-host-protocol`: the v1 types, `API_VERSION`, bounds, and error
  data. The host binary depends on it instead of owning a copy.
- `crates/codegg-sdk`: `HostClient` that spawns or attaches to a host process,
  sends requests, exposes an event stream, cancels and closes.
- Golden wire fixtures for every message.
- A conformance fixture: a scripted host + fake provider used by both the host
  binary's tests and the SDK's tests.
- `docs/embedding.md`: methods, notifications, error classes, versioning,
  security rules and a worked example.

### Explicitly out of scope

- Publishing to crates.io.
- Promoting to a 1.0 semver promise beyond the crate's own version.
- An async runtime choice imposed on consumers beyond what the workspace uses.
- Language bindings other than Rust.
- A second transport.

## 6. Required production changes

### Core/domain

None in the loop or providers.

### Storage and migrations

None.

### Protocol and DTOs

- Move `src/host/protocol.rs` into `crates/codegg-host-protocol`, re-exported by
  the binary. The wire bytes do not change; the move is proven by the golden
  fixtures passing before and after.

### Runtime and concurrency

- `HostClient` owns the child process, a reader task and a request-id counter. A
  dropped client terminates the child and reaps it.
- Events are delivered as a bounded channel so a slow consumer cannot grow
  memory without bound; overflow is a typed client error.

### Frontend or operator surface

None.

### Security and authorization

- The client never accepts a credential on argv; it forwards the descriptor on
  stdin.
- A debug mode prints method names and ids only.

### Documentation and static guards

- `docs/embedding.md` is the contract of record and is linked from the roadmap.
- A guard that the host binary and the SDK import the same protocol crate (no
  duplicate type definitions).

## 7. Ordered work packages

### Work package A — Protocol crate extraction

Intent: one definition of the wire.

Required changes: create `codegg-host-protocol`, move the types, add it to the
workspace, re-export from the binary.

Acceptance evidence: golden fixtures pass before and after; the binary's tests
are unchanged.

### Work package B — Client

Intent: a host can integrate with a few lines.

Required changes: `HostClient::spawn`, `initialize`, `open_session`, `submit`,
`events`, `cancel`, `close`; typed errors that carry the wire class.

Acceptance evidence: a test drives the conformance fixture end to end.

### Work package C — Golden wire and conformance fixture

Intent: compatibility is enforced, not documented.

Required changes: a fixture for every request and notification; a shared
scripted-host harness; a conformance test that fails on any non-additive change.

Acceptance evidence: adding an optional field passes; renaming or removing one
fails.

### Work package D — Documentation and worked example

Intent: a third party can integrate from the docs alone.

Required changes: `docs/embedding.md` with the full contract, the versioning
rule, the security rules and a complete example from `initialize` to
`session/close` including a cancellation.

Acceptance evidence: the example is the same sequence the conformance test runs;
a reviewer not involved in E002 can follow it.

## 8. Failure, cancellation, restart, and contention semantics

- A dropped client kills the child process; no orphan.
- A reader task sees EOF when the child exits; pending requests fail with a
  typed transport error.
- Cancellation is a request, not a kill; the client exposes the completion it
  receives.
- A second `submit` while busy returns the busy class unchanged.

## 9. Compatibility and migration

The crate starts at `0.x` with the wire at `apiVersion: 1`. Additive wire fields
are allowed within 1; any breaking wire change is version 2 and a new major
crate version. The extraction itself changes no bytes.

## 10. Required tests

### Focused unit tests

- Client request/response correlation; event ordering.
- Typed error mapping from wire classes.

### Integration tests

- The conformance fixture through the client: open, submit, events, done, cancel,
  close.
- A slow consumer overflow produces a typed error, not unbounded growth.

### Restart and recovery tests

- Child exit mid-turn fails pending requests and reaps the process.

### Contention and cancellation tests

- Busy refusal; cancel and re-submit.

### Security and negative tests

- No secret in client logs; descriptor goes on stdin.

### Migration and compatibility tests

- Golden fixtures before/after the extraction; an additive-field acceptance and a
  breaking-field rejection.

## 11. Required verification commands

```bash
cargo test -p codegg-host-protocol
cargo test -p codegg-sdk
cargo test -p codegg --bin codegg host
scripts/verify.sh scoped
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 12. Acceptance criteria

1. The host binary and the SDK share one protocol crate.
2. A host written from `docs/embedding.md` can open, submit, receive events,
   cancel and close.
3. Golden fixtures fail on a non-additive wire change and pass on an additive
   one.
4. Client drop reaps the child.
5. No secret appears in a client log or an error.
6. `exec`, ACP and the daemon protocol are unchanged.

## 13. Stop conditions

Stop and re-scope if:

- The extraction cannot be byte-identical.
- The client requires a daemon or the frontend protocol.
- Golden tests cannot distinguish additive from breaking changes.

## 14. Required closure evidence

Closure record under `plans/closure/codegg-embedding-host/003-status.md`:

- the crate layout and the single-definition proof;
- the golden fixture list and the additive/breaking demonstrations;
- the conformance transcript through the client;
- the documentation reference;
- the exact commands and results.
