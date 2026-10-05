# Desktop Frontend and IDE Foundation Milestone 008 — Native LSP Read Surface

Status: ready for handoff

Repository baseline: `f7c8948a`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m006-decomposition`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#87-project-authorization`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted)
- `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted)

Primary class: capability

## 1. Objective

Give the native protocol an LSP **read** surface so a daemon-connected client
can obtain semantic information about a project's files, and make diagnostics
available to a project-scoped subscriber without polling.

This milestone is the transport and authority layer only. It does not render
anything in the TUI; that is M006-C.

## 2. Why this milestone is ready

M006-A, M006-D, and M006-E are closed. ADR-0012 is accepted, having settled
the delivery split, the warm-read policy, and the authorization capability.
Nothing else gates this work.

## 3. Current implementation evidence

- The socket-mode client has no LSP at all: `src/main.rs:3323` builds
  `app.lsp_tool` only under `if !is_socket_mode`, and builds `lsp_service`
  under the same condition at `:3169`.
- The daemon already owns an `LspService` (`src/core/runtime_deps.rs:73`),
  used by `src/core/daemon_documents.rs:12` and `src/core/daemon_turns.rs:421`.
- `codegg-protocol` has exactly one LSP variant pair, `LspPreviewApply` /
  `LspPreviewApplyResult`, and it is a write
  (`crates/codegg-protocol/src/core.rs:2486`, `:1442`).
- `egglsp` already implements every read this milestone exposes, except
  `type_definition`, which does not exist anywhere but a doc comment
  (`crates/egglsp/src/operations/navigation.rs:15`).
- `find_existing_client_for_root_hint`
  (`crates/egglsp/src/service.rs:2375`) resolves an existing client without
  launching one, returning `Err(ServerNotFound)` otherwise. This is the
  warm-read primitive.
- `ProjectionStreamKind::Project` already exists
  (`crates/codegg-protocol/src/projection/replay.rs:34-37`).

**Two traps found during planning, both of which would have failed silently:**

1. `DiagnosticsCollector::get_diagnostics_for_file`
   (`crates/egglsp/src/diagnostics.rs:205`) **debounces** and returns an
   *empty* diagnostic set with `diagnostics_may_still_be_warming: false` when
   called too soon. Using it as the ADR's correctness authority would wipe a
   client's diagnostics on re-pull. The correct read is
   `get_diagnostic_snapshot_for_file` (`:286`), which has no debounce and
   returns the full snapshot with freshness metadata.
2. `MAX_CONNECTION_DIAGNOSTICS = 32`
   (`src/core/transport/projection.rs:24`) is **not** LSP infrastructure. It
   is a connection-local `VecDeque<String>` of lifecycle diagnostic strings
   (`projection.rs:386`). The name coincidence must not be reused.

## 4. Invariants that must not regress

- `src/lsp/mutation.rs` and the `LspPreviewApply` path are byte-identical.
  ADR-0008's write-side authority is untouched.
- The daemon-resolved `LspService` remains the single LSP owner. No second
  service is constructed.
- `authorize_request` remains a preamble before any side effect
  (`src/core/daemon.rs:617-644`); denials emit audit and have zero effect.
- Principal identity comes from the `ClientRegistry` at handshake
  (`src/core/daemon.rs:137-152`), never from a request payload.
- Denial text is non-enumerable and names no `project_id`, `principal`,
  `secret`, `token`, or `bearer`.
- All projection publication routes through `EventLog`'s sink
  (`scripts/check_projection_publication_seam.sh`).
- `ProjectionStreamEvent` never leaks into the raw broadcast
  (`scripts/check_projection_transport_isolation.py`).
- No `mpsc::unbounded_channel` under `src/server/`
  (`scripts/check_websocket_bounds.py`).
- No new role interpretation in `src/core/daemon.rs`
  (`scripts/check_authorization_matrix.py`).

## 5. Scope

### In scope

- `CoreRequest::LspReadGet` — one variant covering hover, definition,
  references, document symbols, workspace symbols, and semantic tokens via an
  operation enum. `via_session` + `file.read`.
- `CoreRequest::LspDiagnosticsGet` — the resync authority. `direct_project` +
  `file.read`.
- `ProjectionEvent::LspDiagnosticsUpdated` on `ProjectionStreamKind::Project`,
  carrying the complete per-file set, a content digest, and a monotonic
  per-file sequence.
- A bounded daemon-side diagnostics poller that publishes on **content change
  only**, tracked by digest.
- Daemon handlers in a new `src/core/daemon_lsp.rs` family module.
- `codegg-client` methods for both requests.
- `operation_descriptor` arms, representative requests, and the
  `architecture/authorization.md` matrix rows.
- Per-file monotonic sequence and digest plumbing so a client can detect a gap
  and re-pull.

### Explicitly out of scope

- **TUI rendering of any kind.** No editor gutter, marker, hover popup, or
  navigation action. That is M006-C.
- `type_definition` — absent from egglsp; new egglsp work, not wiring.
- `code_lens` — implemented in egglsp but unwired anywhere in `src/`.
- Model-facing read tools. ADR-0008 keeps human transport and model-tool
  authorization as separate caller boundaries; this milestone grants neither
  the other.
- Changing the M006-E review surface or `LspPreviewApply` semantics.
- Warming or launching a language server on behalf of a read. ADR-0012
  §3 forbids it.

## 6. Required production changes

### Core/domain

New module `src/core/daemon_lsp.rs`, registered in `src/core/mod.rs`, owning
both handlers and the diagnostics publisher. A new `DaemonRequestFamily::Lsp`
is preferred over extending `daemon_goals.rs` so read and write authority stay
separately readable. `DaemonRequestFamily::of` is exhaustively matched with no
wildcard (`src/core/daemon_family.rs:52`), so routing is forced at compile
time.

A bounded `LspDiagnosticsPublisher` holds, per tracked file: the workspace-
relative path, the last published content digest, and a monotonic sequence
counter. It publishes only when the digest changes.

### Storage and migrations

None. Diagnostics are ephemeral projection state, extending ADR-0008's ruling
that preview/diagnostic state is bounded and non-durable. A `should_persist`
decision is required in `src/core/event_log.rs:37-57` and is made explicitly
rather than inherited.

### Protocol and DTOs

In `crates/codegg-protocol/src/lsp.rs`:

- `LspReadOperation` — `Hover | Definition | References | DocumentSymbols |
  WorkspaceSymbols | SemanticTokens`, externally tagged snake_case, matching
  the existing `CoreRequest` tag style.
- `LspReadRequestDto { operation, session_id, path, line, column, query }` —
  `path`/`line`/`column` present only for point operations; `query` only for
  workspace symbols. Validate at construction.
- `LspReadResultDto` with a per-operation payload enum.
- `LspReadStatus` — `Ready | NotReady`. `NotReady` is a first-class outcome,
  never an error and never an empty success.
- `LspDiagnosticsGetRequestDto { project_id }`,
  `LspDiagnosticsResultDto { files }` where each file carries `path`,
  `sequence`, `digest`, and the complete diagnostic set.
- `LspDiagnosticsProjectionDto { project_id, path, sequence, digest,
  diagnostics, freshness }`.

In `crates/codegg-protocol/src/projection/event.rs`:
`ProjectionEvent::LspDiagnosticsUpdated { project_id, file }`.

In `crates/codegg-protocol/src/core.rs`: the two request variants and their
responses.

### Runtime and concurrency

The publisher runs on a bounded tokio task per active project, driven by an
interval. It reads
`DiagnosticsCollector::get_diagnostic_snapshot_for_file` — the **non-
debouncing** accessor — and computes a content digest over the sorted
`(severity, code, range, message)` tuples so `age_ms`, which changes
continuously, is excluded from the change signal.

Per-project caps: tracked files, publish rate, and payload bytes. Every cap
that fires is recorded in the projection payload as truncation rather than
silently applied.

### Frontend or operator surface

None. `codegg-client` gains typed methods; no TUI wiring.

### Security and authorization

- `LspReadGet` → `ScopeKind::ViaSession` + `Capability::FileRead`, mirroring
  `lsp_preview_apply` = `ViaSession` + `FileModify`
  (`crates/codegg-core/src/authorization/policy.rs:750-754`).
- `LspDiagnosticsGet` → `ScopeKind::DirectProject` + `Capability::FileRead`.
- Both are refused at `ProjectObserve`. ADR-0012 §4 requires this explicitly
  because diagnostics carry source content and the generic projection stream's
  `ProjectObserve` gate understates that.
- Paths are workspace-relative, validated with the existing
  `normalize_relative_path`, and never joined from arbitrary strings.
- The warm-read check calls `find_existing_client_for_root_hint` and maps
  `ServerNotFound` to `LspReadStatus::NotReady`. It must never fall through to
  a create path.

### Documentation and static guards

- `architecture/authorization.md`: two matrix rows.
- `architecture/lsp.md` and `architecture/overview.md`: the new surface.
- `.opencode/skills/` module guide if the module contract changes.
- New guard: a test or script proving every `CoreRequest` LSP read variant has
  an explicit `operation_descriptor` arm and a matrix row. The existing
  `check_authorization_matrix.py` wildcard check is expected to cover this;
  the plan requires **verifying** that expectation rather than assuming it.

## 7. Ordered work packages

### Work package A — Protocol surface

Add DTOs, the two request variants, two response variants, and the
`ProjectionEvent` variant. No behavior. Validate constructors and reject
cross-field misuse (a `query` on a point operation, an empty path).

*Done when:* the crate compiles and DTO validation tests pass.

### Work package B — Authorization

Add both `operation_descriptor` arms, both representative requests, and the
matrix rows. Run `check_authorization_matrix.py` and the policy table-coverage
tests.

*Done when:* the guard passes and a test proves each variant is denied for a
viewer principal and permitted at `file.read`.

### Work package C — Daemon handler family

`src/core/daemon_lsp.rs`, family routing, and the `LspReadGet` handler. The
handler must consult `find_existing_client_for_root_hint` **first** and return
`NotReady` without touching a create path.

*Done when:* a cold read returns `NotReady` and spawns no process, proven by a
test that asserts the client set is unchanged across the call.

### Work package D — Diagnostics publisher and projection event

The digest-keyed publisher, the bounded poller, `CoreEvent` →
`ProjectionEvent` mapping, the `should_persist` decision, and the
`safe_publication` classification.

*Done when:* an unchanged file publishes nothing; a changed file publishes
exactly one envelope carrying the complete set and an incremented sequence;
and publication is refused outside `EventLog`'s sink.

### Work package E — Client methods and resync contract

`codegg-client` methods for both requests, plus the documented gap-detection
rule: a client comparing a streamed `sequence` against the authoritative
`sequence` from `LspDiagnosticsGet` re-pulls on mismatch.

*Done when:* a test drives a gap and asserts the client re-pulls rather than
rendering a partial set.

### Work package F — Documentation, guards, and verification

Architecture docs, skill guide if needed, the new guard, and the full
verification sweep.

## 8. Failure, cancellation, restart, and contention semantics

- **Cold server.** `NotReady`, not an error, not empty. The daemon never
  launches.
- **Cancellation.** The poller is bounded and cancellable; shutdown joins it.
  A hung poller must not hold the daemon.
- **Restart.** Diagnostics are ephemeral. After restart a client has no
  sequence history and re-pulls, which is why the pull is the authority.
- **Contention.** Concurrent reads share the daemon's `LspService`; reads are
  read-only and must not take a mutation lock.
- **Malformed input.** A path that is absolute, contains `..`, or escapes the
  workspace is rejected before any service call.
- **Bounded events.** Tracked files, publish rate, and payload bytes are each
  capped, and firing a cap is reported rather than silent.
- **Queue saturation.** The existing sink drops on full. This is acceptable
  *only* because the ADR makes the pull the authority; a saturation test must
  assert the client can still recover by re-pulling.

## 9. Compatibility and migration

Additive. No existing variant changes meaning. `LspPreviewApply` and
`src/lsp/mutation.rs` are untouched. No storage migration. Rollback is a
revert; the projection event is additive and older clients ignore unknown
events.

## 10. Required tests

### Focused unit tests

- DTO validation: cross-field misuse rejected; empty path rejected.
- Publisher: unchanged digest publishes nothing; changed digest publishes once
  and increments the sequence.
- Digest excludes `age_ms`, so a snapshot that only aged does not republish.
- Every cap reports truncation.

### Integration tests

- `lsp_read_get` via a scripted transport: cold server → `NotReady`; warm
  server → `Ready` with a bounded payload.
- `lsp_diagnostics_get` returns the complete set for a file and the same
  sequence the stream last published.
- A dropped envelope followed by a re-pull converges on the authoritative set.

### Restart and recovery tests

- After a simulated daemon restart, a client with no sequence history
  re-pulls and renders correctly.

### Contention and cancellation tests

- Poller cancellation joins; no task leak.
- A read under projection saturation still recovers by re-pulling.

### Security and negative tests

- Each variant denied for a viewer / non-member / revoked principal, with
  non-enumerable denial text.
- Diagnostics content **refused** at `ProjectObserve` and **permitted** at
  `FileRead`, proving the ADR's divergence from the generic projection gate.
- A traversal path is rejected before any service call.
- A read against a cold server spawns no process (client-key set unchanged).

### Migration and compatibility tests

- `src/lsp/mutation.rs` and the `LspPreviewApply` path unchanged in the diff.
- `check_authorization_matrix.py` and `check_projection_publication_seam.sh`
  green.

## 11. Required verification commands

```bash
cargo fmt --all
cargo test -p codegg-protocol
cargo test -p codegg-core authorization
cargo test --lib lsp
cargo test --test lsp_read_surface_trajectory
python3 scripts/check_authorization_matrix.py
bash scripts/check_projection_publication_seam.sh
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_websocket_bounds.py
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/authorization.md` — two matrix rows plus the rationale for
  diverging from `ProjectObserve`.
- `architecture/lsp.md` — the read surface, warm-read policy, and the two
  traps recorded in §3.
- `architecture/overview.md` — module map entry.
- `plans/registry.md` and the subsystem roadmap — M006-B status.
- `.opencode/skills/` — module guide if the contract changes.

## 13. Acceptance criteria

- A daemon-connected client can obtain hover, definition, references, document
  symbols, workspace symbols, and semantic tokens over the native protocol.
- A read against a cold server returns `NotReady` and spawns no process.
- A project-scoped subscriber receives a complete diagnostics set per file with
  a monotonic sequence and content digest.
- A client that detects a sequence gap can re-pull and converge on the
  authoritative set.
- Both variants are denied for unauthorized principals with non-enumerable
  text, and diagnostics are refused at `ProjectObserve`.
- `src/lsp/mutation.rs` and the `LspPreviewApply` path are byte-identical.
- All guards, clippy, and the full workspace sweep pass locally and hosted.

## 14. Stop conditions

Stop and report rather than improvise when:

- a new LSP variant would need to bypass `DaemonRequestFamily::of`;
- the warm-read policy cannot be honored without launching a server, since that
  would contradict accepted ADR-0012 §3;
- the diagnostics surface cannot avoid the debouncing accessor, since using it
  as the authority would wipe client state;
- authorization would require a wildcard `operation_descriptor` arm, or role
  interpretation in `src/core/daemon.rs`;
- the work would require editing an accepted ADR or an accepted closure record;
- any cap would have to be applied silently rather than reported.

## 15. Documentation and guard obligations

- Every new `CoreRequest` variant must have an explicit `operation_descriptor`
  arm, a representative request, and an `architecture/authorization.md` row.
  A wildcard arm fails the matrix guard.
- Publication of `LspDiagnosticsUpdated` must route through `EventLog`'s sink
  and must not appear in the raw broadcast.
- The new surface must not widen the M006-A editor text-authority guard's
  `SCANNED_FILES` list; this milestone adds no text ownership.
- `MAX_CONNECTION_DIAGNOSTICS` must not be reused or cited as LSP precedent.

## 16. Closure evidence required

- Hosted CI run id with the full sweep count and every guard step green.
- Proof a cold read returns `NotReady` **and** spawns no process, by asserting
  the client-key set is unchanged.
- Proof an unchanged file publishes nothing and a changed file publishes once
  with an incremented sequence.
- Proof the gap-detection re-pull converges, driven end to end.
- Proof diagnostics are refused at `ProjectObserve` and permitted at
  `FileRead`.
- A diff proving `src/lsp/mutation.rs` and the `LspPreviewApply` path are
  unchanged.
- Any test that had to be corrected during implementation, recorded honestly
  rather than presented as product behavior.
