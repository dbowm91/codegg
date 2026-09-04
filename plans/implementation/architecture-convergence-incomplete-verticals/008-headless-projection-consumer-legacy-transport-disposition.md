# Architecture Convergence M008 — Headless Projection Consumer and Legacy Transport Disposition

Status: active

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Closed dependency:

- frontend-neutral session projection subsystem is closed through M012.

Relevant long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4.2-explicit-ownership`
- `plans/000-long-term-specification.md#4.4-frontends-render-projections`
- `plans/000-long-term-specification.md#4.6-progressive-disclosure`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`
- `architecture/protocol.md`
- `architecture/server.md`

Primary class: capability / polish

## 1. Objective

Prove the frontend-neutral session projection contract with a second real non-TUI consumer and use that consumer to audit deprecated frontend transports. Add a small headless/reference client that consumes canonical snapshots/events, handles reconnect/resume, bounded artifact reads, and terminal session/run state without importing TUI state. Then classify deprecated `/ws` JSON-RPC and raw compatibility channels as remove-now, retain-bounded-temporarily, or externally-supported compatibility.

This milestone validates existing architecture; it does not create a new product frontend.

## 2. Explicit non-goals

M008 must not:

- build a web/desktop/mobile UI;
- create a second projection protocol;
- make the headless client a durable state owner;
- duplicate TUI reducers/state merely to pass tests;
- remove compatibility transports without repository/client evidence;
- keep deprecated transports indefinitely merely because tests exist;
- expose sensitive/private projection classes;
- add another server framework or CI lane.

## 3. Current implementation evidence to inspect

Inspect at least:

- `crates/codegg-core/src/projection_replay/` and safe-publication logic;
- `crates/codegg-protocol` projection/session event types;
- local/remote TUI projection adapters/reducers;
- server/WebSocket transport modules, especially `src/server/ws.rs`;
- `architecture/server.md` and `architecture/protocol.md`;
- session-projection closure records, especially frontend adoption/reconnect work;
- artifact handle/read APIs and visibility/redaction rules;
- ACP projection consumption if it currently overlaps this boundary;
- tests/fixtures that still call deprecated `/ws` or raw compatibility channels;
- documentation/scripts/config examples mentioning legacy transport.

## 4. Required headless consumer contract

The reference consumer must be independently runnable in tests and preferably as a small feature-gated/dev binary or library example. It must not depend on ratatui/TUI app state.

It must exercise:

```text
connect/authenticate as supported
subscribe/attach to one session
obtain canonical snapshot
apply incremental projection events
track projection revision/cursor
reconnect/resume after interruption
handle duplicate/replayed events idempotently
read a bounded artifact by handle
observe terminal session/run state
reject/ignore non-public projection content
```

The consumer may print JSON/text for inspection. Visual presentation is irrelevant.

## 5. Ordered work packages

### WP1 — Define minimal reference state

Create a small frontend-neutral client-side state/reducer using public projection types only. It may reuse a canonical reducer if one already exists in a non-TUI crate; it must not copy TUI-specific structs.

### WP2 — Snapshot/incremental integration

Implement snapshot bootstrap and incremental event application. Verify revision/cursor monotonicity and idempotent handling of replayed events.

### WP3 — Reconnect/resume

Simulate or execute a connection interruption, resume from the last accepted cursor/revision, and prove no duplicate durable state or missed terminal transition.

### WP4 — Artifact and visibility path

Use the existing artifact-handle API to retrieve one bounded artifact. Verify safe-publication/redaction policy and ensure the client cannot request raw sensitive/private content outside authorization.

### WP5 — Legacy caller inventory

Inventory all production/tests/docs/scripts callers of:

- deprecated `/ws` JSON-RPC;
- raw session channels;
- UI-specific snapshot/event fallback paths superseded by canonical projections.

Classify each as supported external compatibility, internal compatibility needed for a bounded migration, test-only, dead, or unknown.

### WP6 — Disposition and cleanup

For test-only/dead paths, remove production handlers/adapters where safe and update tests to the canonical projection path. For retained compatibility, document bounds, queue/backpressure behavior, authority limitations, and a concrete removal condition. Unknown callers are a closure blocker until resolved or explicitly treated as supported compatibility.

### WP7 — TUI leakage findings

If the headless consumer cannot express required state without importing TUI-only semantics, fix the projection contract or move the shared reducer/type to the canonical protocol/core owner. Do not patch the headless client with TUI internals.

### WP8 — Documentation

Update `architecture/server.md`, `architecture/protocol.md`, session-projection docs, and developer instructions with the reference consumer and final legacy transport disposition.

## 6. Storage, protocol, migration, compatibility

No storage migration is expected. Protocol changes should be unnecessary if session projection closure is genuinely complete; any required additive change must be narrowly justified by the second-consumer evidence.

Removal of deprecated endpoints is allowed only with caller evidence. If retained, they remain explicitly non-authoritative compatibility paths and must not gain new features.

## 7. Security, backpressure, failure semantics

The reference consumer must obey existing authorization and safe-publication rules. It is not an excuse to expose hidden reasoning, secrets, raw sensitive artifacts, or server-internal state.

Reconnect must be bounded and cancellation-aware. Legacy compatibility paths that remain must retain bounded queues/backpressure and must not bypass canonical publication filtering.

Malformed/out-of-order projection data must produce diagnostics/resync rather than silently corrupt client state.

## 8. Verification

Focused verification must cover:

- snapshot bootstrap;
- incremental update;
- duplicate replay idempotency;
- disconnect/resume;
- terminal state;
- bounded artifact read;
- visibility/redaction denial;
- any removed legacy endpoint no longer reachable by production callers;
- retained legacy path bounded/backpressure behavior.

Use existing server feature-gated integration tests where possible, then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

No browser harness or new CI lane.

## 9. Acceptance criteria

M008 is complete only when:

- a non-TUI consumer exercises canonical projection snapshot + incremental + reconnect/resume end-to-end;
- artifact access and safe-publication behavior are demonstrated;
- no TUI-specific state is required to consume the canonical protocol;
- every deprecated `/ws`/raw compatibility caller has an explicit disposition;
- dead/test-only legacy production paths are removed where safe;
- retained compatibility is bounded, documented, and non-authoritative;
- focused and quick verification pass.

## 10. Stop conditions

Stop and register a corrective session-projection plan if the second consumer exposes a material missing protocol invariant rather than locally patching around it. Do not build a parallel projection protocol.

## 11. Closure evidence required

Record:

- implementation commits;
- reference-consumer scenario evidence;
- reconnect/replay/artifact/redaction results;
- complete legacy caller/disposition matrix;
- removed/retained path list;
- any additive protocol correction;
- verification outcomes and unresolved compatibility risks.
