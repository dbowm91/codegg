# Delegated Run Live Transcript — Roadmap

Status: ready for staged implementation (M001 ready; M002 and M003 blocked on their hard predecessors)
Planning baseline: main at 0be3e0a53ba97ee51f6d64aba2235bdef92452e7 (2026-10-26). Revalidate implementation head before editing.

Long-term references:
- plans/000-long-term-specification.md — architectural principle that frontends render daemon projections; durable agent hierarchy; reliability/recovery; observability; bounded disclosures.
- plans/001-terminology-and-domain-model.md — AgentRunId, delegated run, session, execution authority.
- plans/002-long-term-roadmap.md — session projections, delegated hierarchy and frontend milestones.
- plans/003-planning-process.md §§2–10 — roadmap, handoff, closure and registry governance.

Accepted ADRs:
- plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md — dedicated bounded redacted store, projection only for authorized owning session, continuation by parent.
- plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md — canonical session-owned child lifecycle.
- plans/adrs/ADR-0041-subagent-run-addressing-and-human-interaction-authority.md — parent-mediated steering, no direct user-to-child authority.
- plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md — durable notice and continuation ownership.
- plans/adrs/ADR-0053-daemon-published-projection-surfaces.md — existing daemon-published frontend boundary, not an authorization change.

Predecessor (historical, CLOSED, not reopened): plans/subsystems/delegated-run-observability-and-recovery-corrective-addendum.md (C001–C004, closed 2026-10-24; especially C002 store and C003 snapshot-on-open TUI). The distinction between the C003 recorded transcript and a live-follow buffer is intentional.

## 1. Purpose and ownership boundary

Let a user observe the *recorded, redacted* conversation of a durable subagent while that subagent is running. A new entry should appear in the open TUI tab without manual reopen, including executed tool calls/results and terminal failures. Maintain deterministic recovery after lag, reconnect, cancellation, and restarts. This is an observability feature, not a second execution channel or an exact copy of the provider's internal context window.

The agent loop owns the moment a semantic message or executed tool outcome exists; codegg-core owns the transcript store and redaction-at-write; the daemon owns publication and exact session authorization; codegg-protocol owns bounded typed projection/read contracts; TUI owns display state, cursor, scroll, and error presentation. The frontend never opens the store or subscribes to a foreign run directly.

## 2. Work classification

Invariant:
- A delegated run remains keyed by its durable AgentRunId, not a fabricated session; no child session row, session enumeration change, or parent-session compaction.
- Every displayed body originated from the durable transcript store *after* write-time redaction. Raw provider reasoning, credentials, env values and unbounded tool results never cross a frontend boundary.
- A run is visible only within its originating authorized session, including after the parent turn has ended. Changes to the session binding, principal, subscription or frontend route must invalidate in-flight reads before display.
- Child authority, scheduler ownership, sandbox, worktree/writer fence and ADR-0041 human steering remain unchanged.

Infrastructure:
- M001 captures semantic transcript entries as the child progresses.
- M002 supplies an authenticated, bounded cursor read and a durable-store-derived change hint on the existing session projection publication seam.

Capability:
- M003 renders a near-live transcript in an open TUI tab with gap recovery, honest truncation and stable scrolling.

Polish (included only where necessary): correct loading/absent/old-daemon wording, a manual refresh/retry affordance, and lightweight counters for lag/read failure rather than optimistic activity indicators.

## 3. Explicit non-goals

No raw per-token provider feed, no raw chain-of-thought display, no full-context buffer reconstruction, no unbounded database/event-log retention, no background polling per subagent, no human-to-child messaging, no new scheduler/daemon service or second notification store, no change to AgentRun continuation semantics, no desktop/ACP parity expansion, and no promise that an existing legacy non-durable run has a transcript.

## 4. Measured baseline and defects

- The accepted C002 store is crates/codegg-core/src/agent_run_transcript.rs (SQLite store; append/read with bounds and redaction at write). The store is not a session message table. Its per-row ceiling is 8 KiB, per-run 256 KiB, read limit 256 rows.
- src/agent/worker.rs records the child's system/prompt before agent_loop.run(), but traverses the returned events and retained executed_tool_outcomes to capture model final output, tool calls and tool results *after* agent_loop.run() returns. Thus in-flight inspection often sees just the initial prompt. Failure before the post-run pass can lose the most useful observations.
- C003 reads a transcript through src/core/daemon_refresh.rs and agent_run_summary_with_transcript in crates/codegg-core/src/projection_replay/agent_runs.rs. A run summary's projected preview is at most 64 rows and 8 KiB; the snapshot reads from ordinal 0, so re-fetching that same preview can never provide its later tail once the bound is reached.
- src/tui/app/mod.rs::open_subagent_tab invokes refresh_delegated_transcript only on open. src/tui/commands/primary_projection.rs::refresh_session_snapshot tears down/replaces a subscription for that boundary read. There is no live-follow event, cursor read or coalescing. src/tui/app/render.rs can already render tool calls/results and complete/absent/truncated states.
- Existing session projection and EventLog publication seam, replay/resync machinery, DTO bounds and guards must be reused. In particular do not call ProjectionReplayHandle::publish_core_event from arbitrary workers or fabricate frontend-only liveness.

## 5. Target architecture

A live durable child commits a redacted row when that semantic event occurs, with a monotonic (run id, ordinal) and explicit recording-stopped marker. Only *after* successful commit does a daemon-owned producer publish a session-scoped, body-free transcript-advanced hint via canonical EventLog -> projection seam. The hint identifies run id and committed high-water/recording status, not a raw transcript body. It is advisory: the store and a typed cursor read remain the source of truth.

The TUI fetches bounded transcript pages through CoreClient only for an open, authorized, identity-matching tab. Requests include the owning session and a cursor; the daemon revalidates principal and run-to-session ownership, enforces row/byte limits, and returns authoritative next cursor/has-more/truncation fields. Frontend merges by (run id, ordinal), ignores duplicates/out-of-order hints, detects gaps, catches up from the store and stops cleanly on session switch, tab close, reconnect or detach. Persistent execution may continue without any subscribed clients. A last-page buffer is not represented as the full transcript; the original bounded snapshot remains a compatibility preview.

## 6. Dependency graph

C002 + C003 + ADR-0051 (closed)
             |
             v
M001 live semantic capture (ready)
             |
             v hard
M002 authorized tail read + change signal (blocked pending M001)
             |
             v hard
M003 TUI live follow + end-to-end proof (blocked pending M002)

M002 may design its interface while M001 is active but cannot claim integration closure without M001's committed mid-run entries. M003 may implement isolated UI tests against a fake client after M002's typed interface stabilizes, but cannot close without the real daemon path.

## 7. Milestones

### M001 — Capture the delegated conversation during execution
Primary class: infrastructure. Plan: plans/implementation/delegated-run-live-transcript/001-live-semantic-capture.md.
Exit: while a child is deliberately held at a tool boundary, the store contains its preceding model/tool call/result rows in ordinal order, without waiting for completion; partial failed/cancelled attempts retain committed observations and terminal provenance; pre-existing C002 privacy/bounds and tool execution semantics remain intact.
Status: ready.

### M002 — Session-authorized cursor read and transcript-advanced projection signal
Primary class: infrastructure. Hard dependency: M001. Plan: plans/implementation/delegated-run-live-transcript/002-authorized-tail-and-change-publication.md.
Exit: a subscribed owning-session client receives a body-free advanced hint after commit, and can page the redacted stored tail without a full session snapshot; wrong-session/principal attempts are denied before reading; reconnect, duplicate and gap cases recover from authoritative cursor reads.
Status: blocked until M001 closes.

### M003 — TUI follows open delegated tabs
Primary class: capability. Hard dependency: M002. Plan: plans/implementation/delegated-run-live-transcript/003-tui-live-follow-and-qualification.md.
Exit: the opened TUI tab visibly advances during a paused real child run without reopen, reliably recovers after disconnect, preserves reader scroll and does not mix runs/sessions; closed or hidden tabs impose no recurring live reads.
Status: blocked until M002 closes.

## 8. Cross-cutting requirements

Storage/migration: Prefer no new durable transcript table; use existing (run id, ordinal) and aggregate state. If a revision/high-water index is truly missing, add only the minimal additive migration with explicit restart and old-db qualification.

Protocol/compatibility: Additive version-aware CoreRequest/CoreResponse and projection event. Older clients ignore an unknown hint under the documented protocol-compatibility rules; older daemons leave the existing boundary-read inspector usable. Establish the actual version negotiation path before landing the new event.

Security: Server-side principal/owner authorization is mandatory for cursor reads; a UI selection is not authority. Never put unredacted text in an EventLog hint or frontend cache. Bounds apply to DB write, each cursor response, replay envelopes, and TUI display independently.

Concurrency/cancellation/recovery: Commit before hint. No blocking publication within a store transaction; no unbounded per-token tasks. A dropped/coalesced hint cannot lose content. Preserve ordinals and attempt provenance across continuation; avoid duplicate rows from post-run capture. Close/reconnect/session change invalidates stale requests. Terminal/error paths must produce a queryable consistent transcript.

Performance: Bound worker capture buffering, DB writes, hint frequency, per-tab fetch concurrency, fetch page and in-memory tab buffer; multiple concurrent children must not cause fan-out of full session snapshots.

Observability: Expose bounded diagnostics for denied reads, stale route, overflow/recording stopped and recovery; never log sensitive body text.

Documentation: architecture/agent.md, architecture/projection.md, architecture/tui.md, docs/tui.md and related skill documentation should describe actual behavior after it lands. Retain original closure records as historical evidence.

## 9. Verification strategy

Require mid-execution blocking-fixture tests with SQLite and a real daemon, not only post-completion assertions; forced worker error, cancellation, restart, resumable continuation, duplicate/out-of-order events, two-session isolation, two simultaneous clients, queue saturation and reconnection. Assert redaction against raw SQL storage, the wire and painted TUI; negative tests for authorization and token leakage. Run projection publication, websocket-bound, scheduler-ownership and TUI transport guards with --self-test where provided. Record focused tests, broad verify.sh quick, clippy, and exact-head CI results separately.

## 10. Risks and decision points

- A simple per-token publisher would cause write amplification and secret-bearing event log growth: forbidden. Use bounded semantic checkpoints and body-free hints.
- Re-reading ordinal 0 forever stalls at the existing projection preview ceiling: cursor tail reads are mandatory.
- Capturing only on successful run completion makes the new live view fictional: M001 must prove observations are committed before completion and on failure.
- If existing principal/scope types cannot authorize a run-specific CoreRequest without inventing a new principal or bypass, stop M002 and propose a separate ADR.
- If the current CoreEvent -> projection publication seam cannot carry a typed hint without altering source authority, stop and record a separate architectural decision rather than publishing around the seam.
- The apparent promise of 'complete' means complete within the durable recording bound; never imply access to hidden internal reasoning or unrecorded context.

## 11. Completion definition

The roadmap closes only after M001–M003 have independent closure records, tests validate running-child updates and restart/lag recovery through the real daemon transport, the registry indexes the closures, and operational documentation accurately distinguishes a live recorded transcript from a provider-context buffer.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure |
|---|---|---|---|
| M001 | ready | plans/implementation/delegated-run-live-transcript/001-live-semantic-capture.md | pending |
| M002 | blocked (hard M001) | plans/implementation/delegated-run-live-transcript/002-authorized-tail-and-change-publication.md | pending |
| M003 | blocked (hard M002) | plans/implementation/delegated-run-live-transcript/003-tui-live-follow-and-qualification.md | pending |
