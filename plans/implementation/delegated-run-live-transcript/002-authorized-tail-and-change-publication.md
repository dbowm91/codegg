# Delegated Run Live Transcript M002 — Authorized Tail Reads and Change Publication

Status: blocked — hard dependency M001 live capture closure; interface research may proceed
Primary class: infrastructure
Repository review baseline: main 0be3e0a53ba97ee51f6d64aba2235bdef92452e7, 2026-10-26; revalidate after M001.
Source roadmap: plans/subsystems/delegated-run-live-transcript-roadmap.md §7 M002.
Long-term: plans/000-long-term-specification.md (daemon execution ownership, projection privacy, reliable frontend reconnection); plans/001-terminology-and-domain-model.md; plans/002-long-term-roadmap.md (session projections).
Accepted ADRs: ADR-0051 (read-only redacted projection, exact owner), ADR-0043 (session-owned lifecycle), ADR-0041 (no direct human-to-child control), ADR-0053 (daemon-published surface). Previous closed C003 plan/closure: plans/closure/delegated-run-observability-and-recovery-corrective/003-status.md.

## 1. Objective

Introduce a bounded, session- and principal-authorized cursor API for the durable transcript and a body-free session-scoped transcript-advanced hint on the canonical daemon projection event path. The open TUI can subsequently request only the new redacted tail, without re-subscribing/reloading the entire session or being pinned at the current projection preview's first 8 KiB.

## 2. Dependency readiness

Hard: M001 must prove semantic entries are committed before child completion, on error, and in correct ordinal order. Interface: codegg-core::AgentRunTranscriptStore::read already supports from_ordinal and bounded row count; AgentRunSummary and SessionProjectionSnapshot already carry an initial preview. EventLog already installs the projection sink through daemon_construct; projection adapters/reducer and subscription replay/resync already exist. No new scheduler or store authority is needed. This is not ready for production integration until M001 closes.

## 3. Current implementation evidence

- crates/codegg-core/src/agent_run_transcript.rs::read(run_id, from_ordinal, limit) is a bounded store API; cursor is durable ordinal.
- crates/codegg-core/src/projection_replay/agent_runs.rs::agent_run_summary_with_transcript reads from ordinal 0 and clips to 8 KiB/64 rows. The existing snapshot preview cannot deliver later entries by repeated re-fetch.
- src/core/daemon_refresh.rs::projection_snapshot_for_session enumerates run records using list_by_session(session_id) and attaches preview only for matching owner; cross-session tests in tests/projection_replay/delegated_transcript.inc and src/core/daemon_refresh.rs prove preview isolation.
- src/core/event_log.rs::EventLog is the canonical producer for projection publication. crates/codegg-core/src/projection_replay/publication.rs, crates/codegg-protocol/src/projection/adapters.rs and event.rs/reducer.rs map typed events. scripts/check_projection_publication_seam.sh forbids direct arbitrary publish.
- src/tui/commands/primary_projection.rs::refresh_session_snapshot creates a replacement subscription on open, not an incremental read. M003 will replace that for live-follow, while retaining compatible boundary behavior for older peers.

## 4. Invariants

1. Durable store is the only transcript truth. The change event is an advisory signal *after a committed store append*, containing no body, tool args, credentials or provider reasoning. Lost signals cannot lose rows.
2. An authenticated client can read only a run belonging to its exact authorized owning session, not any run merely guessed by id. A client-provided session id, tab id or child name is not proof of authorization. Revalidate project/workspace/session/principal through the canonical daemon authorization boundary on every read, including observer/read-only semantics.
3. Pagination, serialized body, event and replay window must be bounded independently. An over-limit result is rejected or explicitly truncated with a cursor, never silently marked complete.
4. The run is not a session. No session catalog mutation, new per-run subscription stream, new durable notice store, or scheduler bypass.
5. Publication passes EventLog -> projection seam exactly once; no direct worker call into ProjectionReplayHandle or CoreClient. Existing stream event sequencing/identity and resync contracts are preserved.

## 5. Scope

In: a single typed CoreRequest/CoreResponse for a session-scoped transcript page (or a strictly equivalent existing authorized request if proven), a bounded DTO with status/truncation and authoritative cursor, a body-free changed/advanced projection event with run id and committed high water, daemon publication after successful capture, bounded coalescing/backpressure and tests.

Out: TUI live-follow behavior (M003), raw text in change events, token-level events, client-local DB reads, a new child-session stream, changes to human steering/continuation/scheduler policies, broad refactoring of EventLog.

## 6. Required production changes

Core/domain: use AgentRunId as the read identity, resolve originating session via authoritative durable run/task origin, and read redacted rows exclusively through AgentRunTranscriptStore. Use the store's existing truncation and append outcome; detect absent/recording-stopped versus storage failure separately.

Storage/migration: no new table expected. Query by indexed (run_id, ordinal), verify plan with representative 16-child workload. If a durable high-water checkpoint is required but cannot be derived cheaply and correctly from existing per-run state, add only an additive migration with old-DB qualification; this requires an explicit plan correction.

Protocol/DTOs:
- Define request with session identity, canonical run id, from_ordinal and requested limit; server clamps row and encoded-byte ceilings, validates nonnegative cursor and max sizes, and reports status as absent/present/recording-stopped plus entries, next_ordinal, has_more and source truncation. A full last page must not be mistaken for exhausted history. Distinguish old run with no store data from unauthorized and temporarily unavailable.
- Preserve 64-row/8 KiB AgentRunSummary initial preview as backward-compatible overview. A new page is **not** an unbounded field on AgentRunSummary; do not increase whole-session snapshot bounds to solve the tail.
- Introduce body-free session-scoped transcript-advanced hint whose payload includes run id, committed high-water ordinal/revision and recording-stopped flag, with no text. Explicitly define monotonic semantics; high-water is per-run, not per-child task id. New variants must be version-compatible under the actual CoreEvent/projection negotiation (audit before choosing enum shape).
- Reducer and stream adapters must not invent transcript entries from the hint; they may update a bounded dirty watermark. A mismatch/unsupported variant must degrade to bounded page read on explicit open without breaking older clients.

Runtime:
- Publish a hint only after the corresponding append commits. Wire the production writer to the existing daemon EventLog -> projection seam, not directly to projection replay. Document how worker runtime obtains a constrained typed publisher without introducing per-child global singleton or TUI dependency.
- Coalesce multiple appends per run under a bounded cap/interval or semantic checkpoint; no firehose from every TextDelta. Never hold the transcript SQLite write transaction across EventLog publication; avoid deadlock on full subscriber queues. If delivery is lost or lagged, replay/resync exposes missing state via the page reader.
- Preserve event ordering for two simultaneous children, duplicate notifications and a continuation attempt on the same run id. Canonical storage ordinals dominate any advisory out-of-order watermark.
- Read failure returns a typed error with no partial bodies and no claim that the transcript is absent/complete.

Frontend: none in M002; a minimal non-TUI client/harness must prove the wire contract.

Security/auth:
- Resolve the caller via normal CoreRequest principal/context and check authorized session ownership and read permission. Session binding is checked server-side, including after the parent turn ended; do not require a *live* parent turn for read access. A different principal or session cannot infer body, ordinal count, existence or truncation from a guessed run id.
- Apply per-page byte caps after store redaction and before serialization; do not re-redact at the transport as a substitute for store policy. Avoid logging or echoing bodies on errors.
- A parent authorization to observe a run is not delegated to a remote arbitrary client; stream scope alone is not sufficient without owner checks.

Docs/guards: update architecture/projection.md, architecture/core.md, architecture/codegg_core.md; extend projection disclosure, publication seam and transport-size guards with non-vacuous self-tests for the new variant and page. No raw-body argument in EventLog. Keep the previous C003 snapshot isolation test passing.

## 7. Ordered work packages

A. Security/interface preflight: trace actual principal/auth paths for run detail and owning-session projection; draw a matrix of local owner, team member, read-only observer, unauthorized session, nonexistent run, retired parent turn. Prove the store read bounds and protocol negotiation for unknown events. Stop if the existing API cannot authorize reads without a new cross-boundary policy decision.

B. Typed read slice: implement CoreRequest/CoreResponse with exact-owner check, stable cursor and bounded redacted payload. Tests must pull beyond ordinal 64 and past 8 KiB, distinguishing has_more and recording-stopped; no full session snapshot fetch.

C. Commit-derived publication: add a typed body-free hint through canonical EventLog publication and projection adapter/reducer, emitted only after successful capture write. Prove low-frequency coalescing, per-run monotonic watermark and no frontend body disclosure. Legacy path either remains explicitly without hints or is gated by durable run id.

D. Replay/recovery/negative qualification: test lagged consumers, socket disconnect, daemon restart, two clients/two sessions, duplicate/out-of-order hints and cross-session read denial. Verify producer not tied to an open TUI tab. Record bounds, backpressure, performance and guard evidence.

## 8. Failure, cancellation, restart and contention

Crash after commit before event: persisted tail is recovered by explicit cursor fetch on reconnect; do not promise a hint survived. Crash before commit: no high-water claimed. On saturated subscriber queue: coalesce/mark resync, never block the agent indefinitely or queue unbounded events. On terminal/error/cancellation, persisted rows remain readable; a source-truncated run says so independently of cursor has_more. Concurrent readers of same run receive independent cursors and cannot mutate store; cancellation of one frontend request never cancels the run. A stale session/principal after a project switch is denied regardless of client-side generation fencing. Continuation adds ordinals, never reuses them.

## 9. Compatibility and migration

Old TUI/new daemon: old clients ignore the new event under negotiated protocol or receive a safe existing-compatible event; old overview remains. New TUI/old daemon: M003 must degrade to existing snapshot-on-open behavior and show that live follow is unsupported, not infinitely spin. Existing transcript DBs continue with ordinal 0, no rewrite of historical rows; if a new index/migration is introduced it must be additive. Document exact negotiation behavior rather than assuming serde ignores unknown enum variants.

## 10. Required tests

Focused unit: cursor clamp, overflow/signed invalid values, has_more distinction, run-level truncation, serialized total bound, duplicate/high-water order, event body-free invariant and replay deterministic reduction.

Daemon integration: real owner session and durable run; read after partial append, then after tool completion; page through >64 rows and >8 KiB; assert no repeated first page; wrong session, missing/unauthorized run and read-only observer policy; session A and B each have runs and A cannot read B. Principal-change/read-after-end tests.

Transport: two authenticated clients with separate streams; only owner sees hint and content; duplicate/out-of-order/missed hints recover from store; restart and resubscribe; broadcast lag/resync; queue saturation with concurrent sibling agents. Security: inject known-secret-shaped input into row, inspect raw EventLog payload and serialized API response, prove stored row redacted and hint body-free.

## 11. Required verification commands (run after implementation)

- cargo test -p codegg-protocol --locked -- projection
- cargo test -p codegg-core --locked --lib -- projection_replay
- cargo test -p codegg --locked --lib -- daemon_refresh
- cargo test -p codegg --locked --test projection_replay -- delegated_transcript
- cargo test -p codegg --locked --test agent_loop -- delegated_transcript
- bash scripts/check_projection_publication_seam.sh --self-test
- bash scripts/check_projection_disclosure.sh --self-test
- python3 scripts/check_websocket_bounds.py --self-test
- python3 scripts/check_projection_transport_isolation.py
- python3 scripts/check_projection_transport_lifecycle.py
- python3 scripts/check_execution_ownership.py
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -- -D warnings
- bash scripts/verify.sh quick

Report exact tests run and failures; no closure solely from unit tests.

## 12. Documentation

architecture/projection.md, architecture/core.md, architecture/codegg_core.md; protocol DTO docs and publication contract; guards/skills for projection. TUI operational copy is M003.

## 13. Acceptance criteria

1. A client with correct session/principal can fetch page 2 and page 3 after the existing 8 KiB/64-row preview ceiling with no duplication or missing ordinals.
2. A committed in-flight entry causes a bounded body-free event visible only to the owning authorized session, without full snapshot replay for each entry.
3. A guessed run id from another session/principal reveals neither content nor existence; tests prove server-side denial, not only UI filtering.
4. Dropped event, reconnect and restart can catch up from the store's authoritative cursor and never fabricate completeness.
5. Existing C003 snapshot, disclosure, publication and transport bounds remain green.

## 14. Stop conditions

Stop if authority would be inferred solely from client session id; if a new dedicated scheduler, child-session identity, unbounded stream or direct projection publisher is needed; if protocol negotiation cannot safely support additive variants; if any EventLog body could include raw transcript; if the implementation must violate the closed ADR-0051 disclosure contract. Record proposed ADR and a smaller handoff; do not bypass authorization.

## 15. Closure evidence

Evidence matrix: exact owner/negative-auth cases; complete read/page semantics; store commit-to-hint ordering; replay/gap/saturation trace; wire byte limits and known-secret inspection; old/new peer compatibility; guard self-tests; focused/broad/CI receipts; commits, remaining risks and registry/roadmap status.

## 16. Handoff

Single subsystem owner for daemon/protocol wire changes; review ownership with the M001 capture agent before modifying worker event hooks. Avoid token-level publication. Do not use a frontend-only timer to stand in for missing daemon events.
