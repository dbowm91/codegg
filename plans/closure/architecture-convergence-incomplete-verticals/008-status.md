# Architecture Convergence M008 Closure — Headless Projection Consumer and Legacy Transport Disposition

Status: closed

Implementation commits:

- `69c76ae21f21ef949f5f5382d755bddc925ff65a` — add the production headless projection consumer, integration coverage, caller dispositions, and documentation.
- `04ab35c` — preserve the daemon-issued subscription identity when applying a resync snapshot; reject a resync snapshot that has no usable subscription identity.

## 1. Executive finding

M008 is complete and closed. `codegg_protocol::projection::consumer::HeadlessProjectionConsumer`
is a production, transport-neutral, non-TUI consumer of typed `CoreResponse` projection
messages. It bootstraps a canonical snapshot, applies incremental events through the existing
`ProjectionReducer`, preserves a bounded cursor over disconnect/reconnect, handles duplicate and
gap/replay behavior, observes terminal state, and reads bounded project-scoped artifacts.

The consumer was exercised by a root integration test without importing TUI state. The legacy
transport audit found no in-repository `/ws` caller and no unsupported unknown caller. `/ws` is
retained as explicitly externally-supported, bounded, authenticated compatibility; raw `/tui`
fallback remains bounded, session-scoped, and non-authoritative for a future compatibility
decision; projection-private raw fallback is remove-now behavior already enforced by the
publication seam. No new protocol, storage, scheduler, or CI lane was required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Non-TUI reference state | `crates/codegg-protocol/src/projection/consumer.rs`; no TUI imports or state ownership | Satisfied |
| Snapshot bootstrap and session attachment | `accept_subscribed`, typed attach request, `tests/headless_projection_consumer.rs` | Satisfied |
| Incremental projection application | `apply_event` delegates public events to canonical `ProjectionReducer`; integration replay/live scenarios | Satisfied |
| Cursor/revision monotonicity | Cursor equality, duplicate, gap, session/version checks in consumer; focused tests | Satisfied |
| Duplicate/replayed events | Duplicate sequence numbers are idempotent and surfaced as `Duplicate` | Satisfied |
| Disconnect/reconnect/resume | Retained snapshot/cursor, typed `ProjectionResume`, replay and reconnect integration scenario | Satisfied |
| Terminal session/run state | Terminal turn/run/session events are consumed and asserted in the integration test | Satisfied |
| Bounded artifact access | Project-scoped opaque handles, bounded handle count, validated range, 64 KiB read cap | Satisfied |
| Public visibility and redaction | Internal reasoning, unknown events, diagnostics, and non-public content are not exposed; tests cover private reasoning and oversized artifact rejection | Satisfied |
| Legacy caller evidence and disposition | Matrix in `architecture/server.md` covers `/ws`, raw `/tui`, snapshot-get compatibility, and projection-private fallback | Satisfied |
| Documentation and operating guidance | `architecture/projection.md`, `architecture/protocol.md`, `architecture/server.md`, and `AGENTS.md` updated | Satisfied |
| Verification posture | Focused protocol/headless tests, quick verification, Clippy, projection guards, and relevant transport tests executed | Satisfied |

## 3. Production implementation evidence

The implementation lives in the protocol crate, which is the canonical frontend-neutral
projection owner. It exposes typed requests and response handling for capability negotiation,
subscription, acknowledgement, unsubscribe, resume, artifact listing, and bounded artifact
reads. The state is intentionally limited to one session snapshot, descriptor, subscription
identity, cursor, bounded diagnostics, and bounded artifact metadata.

Incremental events use the canonical reducer rather than copying TUI state. A public event is
accepted only for the attached session and negotiated projection version, and only at the next
cursor. Duplicates are harmless; gaps transition the consumer to resync-required. A resync
snapshot must retain the existing daemon-issued subscription identity; the consumer rejects a
snapshot that cannot be associated with an authenticated subscription.

The integration scenario drives typed capability, subscription, replay, duplicate, disconnect,
resume, terminal transitions, artifact list/read, private reasoning, and oversized artifact
responses. It is deliberately not a TUI adapter and does not exercise raw compatibility events
as projection state.

## 4. Verification executed

Passed or completed without a reported failure:

- `cargo fmt --all -- --check`
- `cargo clippy -p codegg-protocol --all-targets --all-features -- -D warnings`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test -p codegg-protocol --locked` — 168 tests passed
- `cargo test --test headless_projection_consumer --locked -- --test-threads=1`
- `scripts/verify.sh quick`
- `scripts/verify.sh full`
- focused projection replay, artifact, disclosure, controller, and server-transport test targets
- `scripts/check_projection_disclosure.sh`
- `scripts/check_projection_publication_seam.sh`
- `scripts/check_projection_transport_isolation.py`
- `scripts/check_projection_transport_lifecycle.py`
- `scripts/check_websocket_bounds.py`
- `scripts/check_execution_ownership.py`
- `scripts/check_scheduler_bypass.py`
- `scripts/check_identity_path_usage.py`
- `scripts/check_tui_project_authority.py`
- `scripts/check_project_agent_pwd_inference.py`
- `scripts/check_discovery_invariants.py` — 5/5 checks passed
- `git diff --check`

The following repository checks reported unrelated pre-existing baseline findings and were not
caused by M008: `check_daemon_cwd_usage.py` reports two existing Python-script `current_dir`
uses; `check_project_catalog_invariants.py` reports the existing storage-layout expectation
drift; and `check_tool_broker_boundary.py` reports the existing direct review-tool execution
site. These are recorded as low-severity verification limitations, not M008 findings.

## 5. Invariant review

- The daemon, scheduler, durable stores, and publication seam remain the authorities.
- The consumer is observational and does not persist runs, sessions, or projection history.
- Session identity, project scope, negotiated version, subscription identity, and cursor are
  explicit and validated.
- Public projection filtering remains enforced; private reasoning and diagnostic/internal
  content cannot become consumer-visible state.
- Raw compatibility transports do not gain projection authority or a second state model.
- No new process execution, scheduler, provider, Git, storage, or protocol-version owner was
  introduced.

## 6. Failure and recovery review

Malformed or unsupported capability negotiation fails closed. An unattached consumer cannot
apply events or acknowledge a stream. Session/version mismatches, cursor gaps, invalid snapshots,
invalid artifact ranges, unknown handles, and oversized artifact reads produce explicit errors or
resync-required outcomes. Duplicate events are idempotent. Disconnect retains only bounded
reconnect state, and resume requests include snapshot recovery when resync is required.

## 7. Migration and compatibility review

No storage migration, schema migration, or protocol-version change was needed. The complete
legacy disposition is:

| Surface | Evidence | Disposition |
|---|---|---|
| Deprecated `/ws` JSON-RPC route and `RpcRequest` | Server route/handler only; no in-repository production or test client | Retain as externally-supported compatibility; bounded/authenticated and not a projection authority |
| Raw `/tui` event/state fallback | `src/client/attach.rs`, `src/server/ws.rs`, and TUI fallback | Retain temporarily; bounded, session-scoped, and non-authoritative pending a future protocol compatibility decision |
| `CoreRequest::ProjectionSnapshotGet` | Daemon decode/explicit rejection only; no caller | Retain wire compatibility and reject with `projection_snapshot_requires_subscription` |
| Projection-private raw event fallback | No caller; publication/raw forwarders filter it | Remove-now behavior is enforced; private projection envelopes are discarded |

No unknown caller remains unresolved. Removal of `/ws` is intentionally deferred until an
explicit compatibility window and external-client migration evidence exist; the repository does
not infer safe removal from absence of local callers.

## 8. Security review

The consumer assumes authentication is established by its underlying transport and does not
invent an authorization boundary. It validates the negotiated projection version, session scope,
subscription identity, cursor continuity, project-scoped artifact handles, and artifact read
bounds. Opaque handles reject path separators and traversal markers. Non-public projection
classes are ignored or rejected before entering consumer state. The retained transports remain
bounded and non-authoritative, and the existing publication transport-isolation guards pass.

## 9. Documentation and operations

The consumer API and test entry point are documented in `architecture/projection.md` and
`architecture/protocol.md`. The transport caller matrix and compatibility removal condition are
documented in `architecture/server.md`. `AGENTS.md` records the ownership, visibility, artifact,
and legacy-transport rules for future changes. No new operator workflow or CI lane is required.

## 10. Unresolved findings

No critical, high, or medium M008 findings remain.

| Severity | Finding | Disposition |
|---|---|---|
| Low | Existing Python-script `current_dir` guard findings | Pre-existing; outside projection consumer scope |
| Low | Existing storage-layout expectation drift | Pre-existing; outside projection consumer scope |
| Low | Existing direct review-tool execution guard finding | Pre-existing; outside projection consumer scope |
| Informational | External `/ws` client migration evidence is not available in-repository | Explicitly retained as bounded external compatibility; removal requires a future compatibility decision |

## 11. Roadmap disposition

M008's exit condition is satisfied: the session projection contract has a second real production
consumer, the complete caller/disposition matrix is documented, projection-private fallback is
removed by the publication seam, and retained compatibility is bounded and non-authoritative.

The architecture-convergence roadmap is now closed. An audit of `plans/registry.md`, the
roadmap dependency graph, and implementation-plan references found no future registered plan
whose hard dependency or interface dependency was waiting on M008. The only blocked work in the
registry remains runtime-safety C002's historical supported-Linux Landlock fixture evidence,
which is unrelated and remains blocked. Therefore no future plan was unblocked or changed in
this closure.

## 12. Registry updates

- Source implementation plan status changed from `active` to `implemented`.
- The subsystem roadmap status changed from `active` to `closed`.
- M008 is recorded as closed with this closure record.
- M008 was removed from the dependency-ready queue; no implementation plan is currently ready.
- The unrelated runtime-safety C002 blocker remains unchanged.
- No corrective session-projection plan or additive protocol change was required.
