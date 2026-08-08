# Post-Audit Correctness, Simplification, and Footprint Milestone 002 — Daemon Stop Identity and CLI JSON Correctness

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/post-audit-correctness-simplification-roadmap.md`
- Milestone 002

Repository baseline reviewed: `0323d68e0c37c0495540d39ec0d6d9520f124125`

Primary class: lifecycle correctness and CLI correctness

Dependencies:

- hard: none
- soft: none

Target closure record:

- `plans/closure/post-audit-correctness-simplification/002-status.md`

## 1. Objective

Make daemon shutdown target identity-safe and make machine-readable CLI JSON structurally valid for all valid string content.

The milestone should be a small correction to existing lifecycle and serialization paths. It must not create a new daemon control architecture unless the existing protocol cannot provide sufficient identity evidence.

## 2. Explicit non-goals

Do not:

- redesign daemon startup, singleton locking, scheduler ownership, endpoint discovery, or service management;
- introduce a new shutdown protocol family if existing daemon identity can be queried and compared safely;
- add PID namespaces, pidfd-based service management, a supervisor daemon, or platform service abstractions;
- remove the existing SIGTERM behavior when it remains the simplest safe implementation after identity verification;
- change output schemas unrelated to the current JSON wrapper;
- replace clap or broadly refactor `src/main.rs`;
- add new CI lanes or cross-platform service tests.

## 3. Current implementation evidence

Inspect at minimum:

- `src/main.rs` daemon `Stop`, `Status`, and `OutputFormat` handling;
- `src/core/instance.rs` `DaemonPaths`, metadata, lock ownership, and generation semantics;
- `src/core/transport/` client connection and request behavior;
- `crates/codegg-protocol/src/frames.rs` and daemon snapshot response types;
- `tests/single_daemon_lifecycle.rs` and existing CLI/lifecycle tests.

Known defects:

1. `daemon stop` selects a PID from metadata or the legacy PID file, checks only that the configured socket accepts a CodeGG client connection, then signals the selected PID. A stale PID can theoretically be reused by another process while a newer daemon owns the socket.
2. Metadata already contains `daemon_id` and `generation`, while live protocol responses expose daemon identity. The stop path does not currently compare them before signalling.
3. `OutputFormat::Json` performs manual escaping for backslash, quote, newline, carriage return, and tab. JSON requires escaping all U+0000-U+001F control characters, so hand-written output can be invalid for valid input containing other controls.

## 4. Invariants that cannot regress

- The advisory daemon lock remains authoritative for singleton ownership.
- Metadata remains diagnostic and may be stale; stale metadata must never be sufficient on its own to signal a process.
- `daemon stop` must signal only a process whose stored identity is proven to correspond to the live daemon answering the configured endpoint.
- Failure to prove identity must fail closed with actionable diagnostics and no signal sent.
- Endpoint overrides continue to use the existing path-resolution rules.
- A normal current daemon remains stoppable with the same user-facing command.
- Existing daemon protocol versions and request/response schemas remain unchanged if possible.
- JSON output must be produced by a standards-compliant serializer.
- Text output remains unchanged unless correcting an inaccurate diagnostic.

## 5. Preferred daemon-stop design

Use existing identity evidence rather than adding a protocol message.

Preferred sequence:

1. resolve daemon paths/endpoint;
2. read `DaemonInstanceMetadata`;
3. connect to the endpoint;
4. obtain live daemon identity from the negotiated hello or `SnapshotDaemon` using the existing client API;
5. require live `daemon_id == metadata.daemon_id` before signalling `metadata.pid`;
6. optionally compare other stable fields such as protocol generation when they strengthen diagnostics without creating false negatives;
7. send SIGTERM only after identity match;
8. preserve existing best-effort stale PID/socket cleanup behavior only after the target process is proven absent or the metadata is known stale.

Legacy PID-only fallback:

- retain only if another existing source can safely prove that PID belongs to the live daemon;
- otherwise deprecate/fail closed with a diagnostic explaining that legacy PID metadata is insufficient for safe signalling;
- do not inspect arbitrary process command lines as a substitute for daemon identity unless repository/platform constraints leave no better option and tests prove it safe enough.

A protocol-level `Shutdown` request is not required for this milestone. Consider it only if the existing client API cannot expose live daemon identity before stop; if so, stop and document the architectural decision rather than silently expanding scope.

## 6. JSON serialization design

Replace manual escaping with `serde_json` using the smallest typed/value representation that preserves the current external shape.

If the current contract is:

```json
{"response":"..."}
```

preserve that key and value type exactly.

Required cases include:

- quotes and backslashes;
- newline, carriage return, tab;
- backspace and form-feed;
- other ASCII control characters;
- non-ASCII Unicode;
- empty string.

Do not add pretty printing or reorder/change fields merely for aesthetics.

## 7. Expected production-code changes

Likely areas:

- small helper in daemon CLI code or `core::instance` that compares live identity to metadata;
- reuse of existing `SocketCoreClient` request/hello data rather than duplicate socket protocol parsing;
- removal or narrowing of legacy PID-only stop behavior;
- `OutputFormat::Json` implementation switched to `serde_json`.

Keep lifecycle policy in one place. If `Status` and `Stop` duplicate endpoint/metadata probing, a small shared probe helper is acceptable when it clearly reduces code and does not broaden the milestone.

## 8. Ordered work packages

### Work package A — Establish identity contract

1. inspect how `SocketCoreClient` exposes `ServerHello`/`daemon_id` or how `SnapshotDaemon` is requested;
2. confirm metadata `daemon_id` lifecycle and atomic update semantics;
3. enumerate stale metadata/PID/socket states covered by existing tests;
4. select the smallest reusable live-daemon probe.

### Work package B — Correct stop behavior

1. require current metadata for safe PID signalling unless legacy identity can be proven independently;
2. connect and fetch live daemon identity;
3. reject mismatched identities without sending a signal;
4. retain normal successful SIGTERM path;
5. ensure stale cleanup does not delete artifacts owned by a different live daemon;
6. add deterministic PID-reuse/mismatched-metadata tests using spawned fixture processes where practical.

### Work package C — Serializer-backed JSON

1. replace manual string escaping with `serde_json`;
2. preserve the existing object shape;
3. add control-character and Unicode regression tests;
4. verify ordinary text output unchanged.

### Work package D — Focused reconciliation

1. update daemon/client architecture docs only if stop semantics are documented;
2. update CLI output docs/tests if they describe manual escaping or examples;
3. remove dead helper code created obsolete by the change.

## 9. Storage, protocol, migration, and compatibility effects

Storage:

- no schema migration;
- metadata format should remain unchanged.

Protocol:

- no protocol change expected; reuse existing daemon identity.

Compatibility:

- normal daemon stop remains compatible;
- stale legacy PID files that cannot prove identity may now produce a safe failure instead of sending a signal;
- JSON shape remains compatible but becomes standards-correct for all control characters.

## 10. Focused verification

Run focused lifecycle and serialization tests, for example:

```bash
cargo test --test single_daemon_lifecycle
cargo test --bin codegg output_format
scripts/verify.sh quick
```

Use exact selectors that exist after inspection.

Add deterministic tests for:

- metadata/live daemon identity match -> signal path allowed;
- metadata daemon ID mismatch -> no signal;
- stale PID reused by a non-CodeGG fixture -> no signal;
- unreachable endpoint -> no signal;
- control-character JSON parses successfully with `serde_json` and round-trips the original response string.

Do not rely on timing sleeps when process synchronization can use sockets/channels/files.

## 11. Static guards

No new static guard is required.

The lifecycle invariant should be encoded in a helper/API plus tests. JSON correctness is guaranteed by serializer use and regression tests.

## 12. Acceptance criteria

M002 closes only when:

- `daemon stop` proves the live daemon identity matches persisted metadata before signalling a PID;
- mismatched/stale metadata cannot signal an unrelated process;
- safe failure diagnostics identify the mismatch/unverifiable state;
- normal daemon stop behavior remains functional;
- no new daemon authority or service manager is introduced;
- JSON output is generated by `serde_json` or an equivalent standards-compliant serializer;
- arbitrary control-character strings produce parseable JSON and round-trip correctly;
- focused tests and `scripts/verify.sh quick` pass;
- documentation reflects any intentional change to legacy PID fallback behavior.

## 13. Stop conditions

Stop and report if:

- live daemon identity cannot be obtained through existing protocol/client APIs without a wire change;
- metadata `daemon_id` is not stable for the daemon lifetime as currently documented;
- safe PID signalling requires a broader cross-platform process-management design.

Do not solve those by trusting PID or socket reachability alone.

## 14. Required closure evidence

`plans/closure/post-audit-correctness-simplification/002-status.md` must include:

- implementation commit/PR;
- exact identity proof used before signalling;
- stale/mismatch test outcomes;
- JSON round-trip/control-character test outcomes;
- focused verification commands;
- compatibility/operational notes;
- unresolved findings by severity.
