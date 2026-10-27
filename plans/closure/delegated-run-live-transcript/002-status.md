# Delegated Live Transcript Milestone M002 — Closure Status

- **Plan**: `plans/implementation/delegated-run-live-transcript/002-authorized-tail-and-change-publication.md`
- **Roadmap**: `plans/subsystems/delegated-run-live-transcript-roadmap.md` §7 (M002)
- **Branch**: `plan/delegated-run-live-transcript`
- **Status**: closed

## 1. Executive finding

A live transcript needs two things M001 did not have: a way for a client to
**read past the initial preview** under an authority the daemon actually
checks, and a way for the daemon to say **"there is more"** without putting
the transcript on a broadcast stream.

Both exist now.

The read is a typed, session-scoped page:
`CoreRequest::AgentRunTranscriptRead { session_id, run_id, from_ordinal, limit }`
→ `CoreResponse::AgentRunTranscriptRead { page }`. The daemon re-resolves the
run's **own** originating session from the durable run/task origin before it
reads a row, so a client-supplied session id is a claim that gets checked,
never a grant. The page answers with the store's authoritative
`next_ordinal`, so a client walks past the 64-row preview without
re-fetching the session.

The change signal is a **body-free advisory**:
`CoreEvent::AgentRunTranscriptAdvanced { session_id, run_id, ordinal,
recording_stopped }`. It carries no text, no tool arguments, no reasoning and
no credentials, and it is published **only after** the corresponding store
append commits. The reducer records a bounded, monotonic, per-run **dirty
watermark** and nothing else: it never invents a transcript entry, and it
never claims the transcript is complete. The canonical cursor always comes
from the store.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The read is authorized server-side against the run's own durable owner | `src/core/daemon_refresh.rs::handle_agent_run_request` — `task.originating_session_id != session_id` → not-found; `crates/codegg-core/src/authorization/policy.rs` — `ScopeKind::ViaSession` + `Capability::SessionRead` | pass | The descriptor cannot express exact-ownership, so the handler owns that gate |
| A foreign or unauthorized read cannot probe existence | `tests/core_daemon/delegated_transcript_m002.inc` — `a_granted_principal_reading_another_session_run_gets_the_not_found_refusal`, `a_principal_without_a_grant_is_denied_before_the_run_is_consulted` | pass | A foreign session gets the same answer as a missing run; a store read failure is answered like a missing run |
| The page is bounded independently of the preview | `crates/codegg-protocol/src/projection/limits.rs` — `MAX_PROJECTION_TRANSCRIPT_PAGE_ROWS` / `..._PAGE_BYTES`; `AgentRunTranscriptPageDto::checked` | pass | Over-bound pages are **refused**, not shortened |
| The page carries an authoritative cursor, and a full last page is not exhausted history | `AgentRunTranscriptPageDto { next_ordinal, has_more, source }`; `AgentRunTranscriptPageStatus::{Absent, Present, RecordingStopped}` | pass | `has_more` and `recording_stopped` are separate facts |
| The tail past the preview ceiling is reachable | `tests/core_daemon/delegated_transcript_m002.inc` — `the_tail_past_the_preview_ceiling_is_reachable_by_cursor` | pass | The 64-row preview is not the whole transcript and the cursor reaches beyond it |
| The advisory is body-free | `tests/projection_replay/delegated_transcript_advance.inc` — `the_advisory_carries_no_transcript_body` | pass | No text, no arguments, no reasoning, no credentials |
| The advisory is published only after the append commits | `src/agent/transcript_capture.rs` — `announce` runs after `append` returns `Ok`; `a_committed_append_announces_the_advance`; `a_failed_write_announces_no_ordinal_the_store_did_not_commit` | pass | A failed write announces nothing |
| The producer reaches the daemon's canonical EventLog seam | `src/agent/transcript_publisher.rs`; `src/core/daemon_construct.rs` installs the publisher into the pool it owns | pass | No global singleton, no TUI dependency, no direct `ProjectionReplayHandle` call |
| The reducer invents no transcript and never claims completeness | `tests/projection_replay/delegated_transcript_advance.inc` — `the_reducer_records_a_watermark_and_invents_no_transcript` | pass | The watermark is advisory metadata only |
| A duplicate or out-of-order advisory is idempotent | `AgentRunTranscriptSnapshot::upsert_transcript_watermark` (monotonic per run); `a_duplicate_advisory_is_idempotent` | pass | The canonical storage ordinal dominates any advisory watermark |
| The producer is not tied to an open TUI tab | `tests/projection_replay/delegated_transcript_advance.inc` — `the_advisory_reaches_the_projection_stream_with_no_tui_attached` | pass | The advisory reaches the stream with no client attached |
| The run is not a session: no session catalog mutation, no new stream, no scheduler bypass | `DaemonRequestFamily::AgentRuns` is a request family, not a subscription stream; the store is unchanged | pass | No new per-run subscription, no new durable notice store |
| The guards cover the new variant and page non-vacuously | `scripts/check_projection_disclosure.sh --self-test` (4 cases), `scripts/check_projection_publication_seam.sh --self-test` (5 cases), `scripts/check_websocket_bounds.py --self-test` (1 case) | pass | Every injected defect is rejected |

## 3. Production implementation evidence

### Protocol — `crates/codegg-protocol/`

| Item | Role |
|---|---|
| `CoreRequest::AgentRunTranscriptRead { session_id, run_id, from_ordinal, limit }` | The typed page read. The cursor is the store's own durable ordinal; negatives clamp to zero and `limit` clamps to the declared page ceiling |
| `CoreResponse::AgentRunTranscriptRead { page }` | The bounded page |
| `CoreEvent::AgentRunTranscriptAdvanced { session_id, run_id, ordinal, recording_stopped }` | The body-free post-commit advisory |
| `AgentRunTranscriptPageDto` | `run_id`, `status`, `entries`, `next_ordinal`, `has_more`, `source` |
| `AgentRunTranscriptPageStatus` | `Absent` / `Present` / `RecordingStopped` — three different facts, and `Absent` is never a rendering of "unauthorized" |
| `AgentRunTranscriptSourceState` | `{ recording_stopped, total_entries }` — the store's own record, independent of `has_more` |
| `AgentRunTranscriptWatermarkProjection` | The advisory watermark: `run_id`, `ordinal`, `recording_stopped` |
| `ProjectionEvent::AgentRunTranscriptAdvanced` | The wire form; the adapter copies the run id, the ordinal and the flag and never reads a body |
| `SessionProjectionSnapshot.transcript_watermarks` | The bounded, monotonic, per-run dirty watermark the reducer maintains |

### Authorization — `crates/codegg-core/src/authorization/policy.rs`

The new request's descriptor is `ScopeKind::ViaSession` with
`Capability::SessionRead` — **not** `SessionObserve`. The page carries the
child's recorded conversation, which is content; `session.observe` is the
narrower "may see that this session exists and how it is going" capability,
and a principal holding only that is refused the body. The exact-owner check
is a second, handler-level gate, because the descriptor cannot express it:
the run id is the thing being claimed, not the scope.

`DaemonRequestFamily::AgentRuns` is its own family precisely because the
handler's authority is the run's **durable ownership**, not the session's.

### The handler — `src/core/daemon_refresh.rs`

1. A run id that does not parse cannot name a run, so it gets the same
   answer as a run that does not exist.
2. The run's own task origin is the authority: `task.originating_session_id
   != session_id` → not found.
3. A **store read failure** is answered like a missing run, so a failing run
   store cannot be used to probe which run ids exist.
4. The read is bounded three ways, independently: the store's own read limit,
   the declared page row ceiling, and the declared byte ceiling applied to
   the already-redacted bodies.
5. An over-bound page is **refused** with `transcript_bound_exceeded`, not
   shortened into something that reads as the whole transcript.

Ownership is session-scoped and durable: a retired parent turn does not close
the door.

### The publisher — `src/agent/transcript_publisher.rs`

A constrained typed handle, `TranscriptAdvancePublisher`, with one method.
It is **injected**: the daemon that owns the subagent pool installs it into
the pool, and the delegated child's runtime receives it. There is no global
registry and no lookup at write time, and the module has no TUI dependency.

The capture sink (`src/agent/transcript_capture.rs`) calls `announce` **after**
`append` returns `Ok`, so a hint can never claim a row the store does not
hold. A failed write announces nothing. The store's write transaction is
never held across the publication.

## 4. Verification executed

### Commands run

```bash
cargo test --test core_daemon -- delegated_transcript
cargo test --test projection_replay -- delegated_transcript
bash scripts/check_projection_disclosure.sh --self-test
bash scripts/check_projection_publication_seam.sh --self-test
python3 scripts/check_websocket_bounds.py --self-test
```

### Results

| Command | Result |
|---|---|
| `cargo test --test core_daemon -- delegated_transcript` | 4 passed, 0 failed |
| `cargo test --test projection_replay -- delegated_transcript` | 10 passed, 0 failed (5 new `delegated_transcript_advance` members) |
| `scripts/check_projection_disclosure.sh --self-test` | 4 injected defects rejected |
| `scripts/check_projection_publication_seam.sh --self-test` | 5 injected defects rejected |
| `scripts/check_websocket_bounds.py --self-test` | the transcript bound check rejects an over-bound and an undeclared bound |
| `scripts/check_audit_coverage.py` | all audit coverage invariants verified — the new operation is classified |
| `scripts/check_projection_publication_seam.sh` | no unauthorized direct projection replay publication sites; the delegated run transcript crosses only on the session projection snapshot |
| Every guard in `scripts/verify.sh quick`'s `run_guards()` (82 guards) | 0 failures |

The full workspace sweep (`cargo nextest run --workspace --locked --profile ci --no-fail-fast`)
runs 16,600 tests: **16,569 passed**, 30 failed, 1 timed out, 9 skipped. Every
one of the 31 non-passing tests was checked against the **baseline** (this
branch's `HEAD` with this line's changes stashed): **29 of the 30 failures
reproduce identically there**, and the thirtieth passes on its own in this
tree. None of them is in this milestone's surface. The detail is recorded in
M001's closure record §4.

This milestone's own surface is green: `cargo test --test core_daemon -- delegated_transcript`
is **4 passed**, and `cargo test --test projection_replay -- delegated_transcript`
is **10 passed** (5 of them the new `delegated_transcript_advance` members).

## 5. Invariant review

| Invariant | How it holds |
|---|---|
| No transcript body on a broadcast stream | The advisory carries a run id, an ordinal and a flag. The body-free property is asserted by a test that reads the event |
| The canonical cursor comes from the store | `next_ordinal` is the store's committed ordinal. The watermark is advisory and never used as a cursor |
| A client-supplied session id is a claim | The daemon resolves the run's own originating session and compares |
| No existence oracle | A foreign run, a malformed run id, a missing run and a failed store read all produce the same answer |
| The reducer invents nothing | The reducer records a watermark and no entry |
| The producer is not tied to a TUI tab | The advisory is published by the daemon's own capture path; the test asserts it reaches the stream with no client attached |
| No new session identity | The run is still a run. No session row, no per-run subscription stream, no scheduler bypass |
| No unbounded queue | The watermark deque is bounded by `MAX_PROJECTION_TRANSCRIPT_WATERMARKS` |

## 6. Failure and recovery review

| Scenario | Behaviour |
|---|---|
| The store read fails | Answered `transcript_unavailable`, carrying no body and no ordinal count |
| The page is over the declared bound | Refused with `transcript_bound_exceeded`; the client is told to re-read with a smaller cursor step |
| The advisory is duplicated or arrives out of order | The watermark is monotonic per run; a stale hint never moves it backwards |
| The advisory is dropped | The client's next page read recovers from the authoritative cursor; the watermark is only a trigger |
| The daemon restarts | The durable store is the truth. A reconnecting client re-reads and reconciles its cursor from the page, not from the last hint |
| A continuation reuses the run id | The watermark is per **run**, so a continuation advances the same watermark rather than starting a new one |

## 7. Migration and compatibility review

No schema migration. The store's tables and bounds are unchanged.

The protocol version is **not** bumped: `PROJECTION_PROTOCOL_VERSION` stays at
1 and `PROJECTION_CAPABILITY` stays `session_projection.v1`. The transport
forwards an undecodable frame by **skipping** it with a warning
(`crates/codegg-client/src/local.rs`, `src/core/transport/daemon_socket.rs`),
and the legacy `/tui` path drops a `CoreEvent` it has no arm for
(`src/server/ws.rs`, `convert_core_event_to_tui`). An older client therefore
sees the new advisory as an unknown variant and ignores it — the advisory is
advisory, so ignoring it costs a client nothing but the extra fetch it would
have done anyway. The **initial preview** on `AgentRunSummary` remains valid
against an older daemon, and the pre-existing 64-row / 8 KiB preview is
unchanged.

The advisory is a **new variant** and not a new field on `AgentRunSummary`,
because a new field would change an existing variant's shape — and a changed
shape is the one thing that would make an older consumer mis-parse rather
than skip.

## 8. Security review

| Property | Where it is enforced |
|---|---|
| Server-side authorization | `operation_descriptor` (`ViaSession` + `SessionRead`) plus the handler's exact-owner gate |
| No existence oracle | The handler answers a foreign, malformed, missing and unreadable run identically |
| Bodies only through the authorized read | The advisory carries no body; the page is the only body-carrying surface and it is gated |
| Redaction preserved | The page serves the store's already-redacted bodies; the byte ceiling is applied after redaction and before serialization |
| A read-only observer | `session.observe` is refused the body; `session.read` is required |
| No credentials on the stream | The advisory carries no credential-shaped field, and the guard self-test rejects a body-bearing advisory |

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/projection.md` | The advisory, its body-free contract and the dirty watermark |
| `architecture/core.md` | The new request family and its authorization |
| `architecture/codegg_core.md` | The page reader and its clamps |

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The advisory is published once per committed append, so a chatty child produces one advisory per row | The client coalesces them, and the producer is bounded by the run's own byte bound | A future milestone may coalesce at the producer; the client's coalescing already covers it |
| low | The page's `Absent` status and the store's absence are indistinguishable from "the run predates the transcript store" | A reader shows "no recorded transcript", not an error | None: the state is honest as it stands |

No finding blocks the milestone.

## 11. Roadmap disposition

M002 is closed. M003 (the TUI live follow) consumes this milestone's typed
read and its advisory.

## 12. Registry updates

`plans/registry.md` records M002 as closed and M003 as closed.
