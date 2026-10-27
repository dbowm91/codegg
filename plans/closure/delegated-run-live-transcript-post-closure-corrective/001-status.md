# Delegated Run Live Transcript Post-Closure Corrective Milestone C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/delegated-run-live-transcript-post-closure-corrective/001-progress-and-advisory-liveness.md`

Source subsystem roadmap:

- `plans/subsystems/delegated-run-live-transcript-roadmap.md#13-post-closure-integration-tracking-new-corrective-line`
- `plans/subsystems/delegated-run-live-transcript-post-closure-corrective-addendum.md`

Repository baseline reviewed: `af7f1ff57` (branch `plan/delegated-run-live-transcript`, in sync with
`origin/plan/delegated-run-live-transcript`)

Implementation commits or pull requests:

- pending — see the branch tip for the C001 implementation commit (producer flush worker, TUI pass
  lifecycle, composed proofs and this record).

## 1. Executive finding

Both measured liveness defects are closed in production code, and both are closed by evidence rather
than by narration.

**L1 — the four-page catch-up dead-end.** The delegated tab's catch-up pass consumed at most four
pages and then stopped for good. The page budget belonged to the *tab* rather than to the *pass*, so
a transcript longer than four pages left the cursor stuck (measured: 256 of 384 rows) with the daemon
holding the rest, and neither a focus nor an advisory moved it. The pass now has a lifecycle: a pass
reads at most four pages, and when the store still holds rows the tab **yields** — it schedules the
next bounded pass through the TUI's command channel, so the event loop serves whatever else is
waiting before the next pass reads another page. A tab therefore catches up to `has_more = false` on
its own, with no new hint, no retry and no refocus.

**L2 — the quiet-run advisory deadline.** The producer coalesced a run's appends under a row cap and
an interval, but the interval was only ever consulted *at append time*, and nothing scheduled a wake.
A run that committed one to seven rows and then went quiet — a child blocked inside a tool — held
those rows until the next append, the run's recording-stopped state, or the run's completion. A
follower that had already read the run's first high-water had no reason to read again, so the
watermark simply never arrived. The producer now owns **one** bounded flush worker: it waits for the
earliest deadline at which a coalesced advance becomes due, publishes what is due, and waits again.
A quiet run's committed watermark reaches its subscribers within one interval of the run's last
published advisory, with no further append and no run completion.

The two fixes are independent by construction: the producer's timer is a *producer* mechanism and
the TUI still has no timer of its own.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A >4-page authorized transcript catches up to `has_more = false` in one open active tab via bounded yielding, with no new hint, retry or refocus | `src/tui/commands/transcript_follow.rs::a_long_transcript_catches_up_across_more_than_four_pages` (fake client, 6 pages) and `::the_tab_catches_up_a_real_daemons_long_transcript` (real daemon, real SQLite store, 384 rows) | pass | Both assert the cursor reaches 384, that the read cursors walk forward one page at a time (`0, 64, 128, 192, 256, 320`), that the peak in-flight reads is **one**, and that the catch-up took more than one pass |
| Each pass remains ≤ 4 pages | `TranscriptCache::pages_in_pass` reset in `begin_pass`; the pass boundary is the only place a new pass begins; `MAX_TRANSCRIPT_PAGES_PER_PASS` unchanged at 4 | pass | The budget was not raised; the plan §6 invariant "the per-pass limit of 4 caps each processing cycle, not the tab's lifetime" is what changed |
| Each tab ≤ 1 outstanding request | `FakePageClient::peak_in_flight` assertion in the catch-up test | pass | Unchanged per-tab request id fence |
| 1–7 new committed rows followed by a quiet blocked child produce an event for the highest committed ordinal within a documented bounded period, **without further appends** | `src/agent/transcript_publisher.rs::a_quiet_run_publishes_its_coalesced_advance_without_a_further_append` (unit) and `::the_advisory_crosses_the_event_log_from_a_real_durable_run` (real durable run, real event log) | pass | The bound is `ADVANCE_INTERVAL` (500 ms default) after the run's last published advisory |
| The producer publishes while the child is still blocked | `tests/agent_loop/delegated_transcript.inc::the_daemon_publishes_the_advance_while_the_child_is_still_blocked` | pass | A real child blocked in a FIFO-backed `read` tool; the advisory reaches the daemon's canonical event log while the task is still `Running` |
| No body appears on the event path | `TranscriptAdvance` carries only `{ run_id, ordinal, recording_stopped }`; `EventLogTranscriptAdvanceSink` copies only those fields; `check_projection_publication_seam.sh` passes | pass | No text, no tool arguments, no reasoning, no credentials |
| Mixed 16-run concurrency creates no unbounded producer/task state and no runaway TUI fetches | `src/agent/transcript_publisher.rs::sixteen_concurrent_runs_stay_within_the_publication_bounds`; `MAX_TRACKED_RUNS = 256` with oldest-first eviction; a finished run's entry is dropped at its completion flush | pass | One worker per publisher, never one per append; the tracked map is capped |
| The source is authoritative when events drop | The durable store remains the only source of rows; a lost advisory is recovered by a cursor read; the tab's `next_ordinal` is the store's authoritative cursor | pass | Unchanged from M002/M003 |
| Route/session switches, reconnect and close/reopen never display stale foreign rows | The existing fences are unchanged (request id → route token → run id → cursor → session → project); `a_completion_for_a_superseded_request_is_discarded` still passes; `::a_reconnect_drops_the_fetched_rows_and_the_tab_reads_them_again` proves the reconnect crossing end to end against a real store | pass | Re-read authorization, redaction, scroll and eviction behaviour are unchanged |
| Deterministic tests **fail on baseline** for both defects, pass with the corrections | Baseline run in a detached worktree at `af7f1ff57` with the new test module spliced over the pre-fix production code | pass | See §4 for the exact failures |
| A real-daemon composition receipt | `::the_tab_catches_up_a_real_daemons_long_transcript`, `::the_daemons_advisory_drives_the_tuis_live_follow` (real daemon → real store → real projection reducer → real TUI hint → painted row) | pass | The second one is C002-C's composed chain and is recorded in the C002 record as well |

## 3. Production implementation evidence

### Producer — `src/agent/transcript_publisher.rs`

- `CoalescingTranscriptPublisher` was restructured around a shared `CoalescerState` (a map of
  tracked runs plus a FIFO of tracked run ids) behind one `Mutex`, an injectable `AdvanceClock`, and
  a `watch` channel that carries the wake.
- A **single** flush worker is started lazily, once per publisher, the first time a run holds a
  coalesced advance. It waits for the earliest deadline at which a coalesced advance becomes due
  (`last_published_at + ADVANCE_INTERVAL`), publishes what is due, and waits again. The worker holds
  no state of its own and ends when the publisher is dropped (the wake channel closes with it).
- The coalescing lock is never held across an `await`: the due advances are collected under the lock
  and published outside it.
- `should_publish` stays a pure function; `is_due` was extracted from it so the *policy* — when a
  coalesced advance is due — is testable without a wall clock.
- The producer never publishes an ordinal below the watermark already published for the run: the
  canonical storage ordinal dominates the merge, and a pending advance that carries nothing new is
  dropped rather than published twice.
- The tracked map is bounded (`MAX_TRACKED_RUNS = 256`, oldest-first eviction) and a finished run's
  entry is dropped at its completion flush, so a long-lived daemon does not accumulate one entry per
  delegated run.
- The advisory's shape is unchanged: `{ run_id, ordinal, recording_stopped }`, published through the
  daemon's canonical `EventLog` and nowhere else.

### Consumer — `src/tui/commands/transcript_follow.rs`, `src/tui/app/state/transcript_cache.rs`

- `TranscriptCache` gained a pass lifecycle: `begin_pass` (a new pass resets the page budget),
  `schedule_pass` (claim the right to schedule one pass; a second claim is refused so two passes
  cannot read the same page) and `cancel_pass` (release a claim that never reached the event loop).
- `start_follow` no longer dead-ends when the pass budget is spent: it marks the tab
  `CatchingUp` and **schedules the next bounded pass**.
- `apply_page` schedules the next pass at the pass boundary instead of returning with rows still
  pending, and it now stops a **hidden** tab from continuing to read: only the focused tab reads
  eagerly.
- A completion frees a slot in the fair read budget, so a completion also wakes the focused tab
  that was waiting for one — the waiting tab is not left in an indefinite no-op.
- `on_tab_focused` begins a new pass: a focus is a tab boundary and therefore a new catch-up cycle.
- A new internal command, `TuiCommand::SubagentTranscriptPass { tab_id }`, carries the scheduled
  pass through the existing command/event loop. It is dispatched by
  `crate::tui::runtime::command_dispatch::dispatch_tui_command`, so the pass boundary is a turn of
  the loop rather than a recursive read.
- `TranscriptFollowStatus::CatchingUp` is now the status of a tab that is still behind the store.
  `status_for` returns it whenever the page says `has_more`, so the tab never claims to follow the
  live edge (or to be complete) while the source holds more.

### What did **not** change

- No TUI timer, no poll, and no read that is not caused by a hint, a tab boundary or a terminal run.
- No change to the row cap, the interval, the cache bound, the page bound, the request bounds or the
  authorization rules.
- No change to the advisory's fields, the projection reducer, the store, the daemon's read path, the
  loop's control flow or the scheduler.

## 4. Verification executed

### Commands run

```bash
# Baseline red — a detached worktree at the pre-fix commit, with the new
# test module spliced over the pre-fix production code.
git worktree add /tmp/codegg-c001-baseline af7f1ff57
cd /tmp/codegg-c001-baseline
CARGO_TARGET_DIR=<worktree>/target cargo test --lib -- transcript_follow
CARGO_TARGET_DIR=<worktree>/target cargo test --lib -- transcript_publisher

# The corrected tree.
cargo test --lib -- transcript_follow
cargo test --lib -- transcript_publisher
cargo test --test agent_loop -- delegated_transcript
cargo test --test tui_render -- transcript_follow
cargo test --test tui_render -- delegated_transcript
cargo test --test core_daemon -- delegated_transcript
cargo test --test projection_replay -- delegated_transcript
cargo fmt --all -- --check
python3 scripts/check_delegated_transcript_store.py --self-test
bash scripts/check_projection_publication_seam.sh --self-test
bash scripts/check_projection_disclosure.sh --self-test
python3 scripts/check_tui_transport_authority.py --self-test
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_authorization_matrix.py
python3 scripts/check_websocket_bounds.py
python3 scripts/check_audit_coverage.py
python3 scripts/check_test_target_references.py
python3 scripts/check_adr_integrity.py
python3 scripts/check_comment_budget.py
bash scripts/verify.sh quick
```

### Results

**Baseline red (pre-fix production code at `af7f1ff57`)** — the new test module was spliced over the
pre-fix production code, so the failures are the defects and not a compile error:

| Test | Baseline result |
|---|---|
| `a_long_transcript_catches_up_across_more_than_four_pages` | **FAILED** — "the tab must catch up to the store's cursor, got 256" (expected 384) |
| `the_tab_catches_up_a_real_daemons_long_transcript` | **FAILED** — "the tab must catch up to the real store's cursor, got 256" (expected 384) |
| `a_tab_that_loses_the_focus_stops_reading` | **FAILED** — "a tab that lost the focus must not keep reading the store" |
| `a_quiet_run_publishes_its_coalesced_advance_without_a_further_append` | **FAILED** — "the quiet run's coalesced advance must be published within the bound: `Elapsed(())`" |

The remaining 7 spliced baseline tests passed, so the splice itself was sound: the four failures are
the four defects.

**Corrected tree:**

| Suite | Result |
|---|---|
| `cargo test --lib -- transcript_follow` | 13 passed, 0 failed |
| `cargo test --lib -- transcript_publisher` | 13 passed, 0 failed |
| `cargo test --lib -- transcript` (every transcript-named unit test) | 61 passed, 0 failed |
| `cargo test --test agent_loop -- delegated_transcript` | 11 passed, 0 failed (including the new blocked-child composition) |
| `cargo test --test tui_render` (whole family) | 1109 passed, 0 failed |
| `cargo test --test core_daemon -- delegated_transcript` | 4 passed, 0 failed |
| `cargo test --test projection_replay -- delegated_transcript` | 10 passed, 0 failed |
| The twelve guard scripts above | all pass |
| `cargo fmt --all -- --check` | clean |
| `bash scripts/verify.sh quick` | green — "Quick verification passed." |

`cargo clippy --workspace --all-targets --locked -- -D warnings` was also run on the **CI
toolchain** (`+1.99.0`) and reports **0 errors**; that receipt belongs to the C002 record.

## 5. Invariant review

| Plan invariant (§4/§6) | Evidence |
|---|---|
| At most one page request per tab | `FakePageClient::peak_in_flight() == 1` in the catch-up test; the per-tab `begin_request`/`finish_request` fence is unchanged |
| Fair global outstanding limit (4) | `MAX_TRANSCRIPT_FETCHES_PER_PASS` unchanged; a completion now also wakes the focused tab that was waiting for a slot |
| The per-pass limit of 4 caps each *processing cycle*, not the tab's lifetime | `begin_pass` resets the counter when a new pass begins; `schedule_pass`/`continue_pass` move the boundary through the command channel |
| The cache's `next_ordinal` is the only authoritative cursor | Unchanged; the pass lifecycle never invents a cursor |
| The advisory carries only (owner session, run id, ordinal, recording_stopped) | `TranscriptAdvance` and `EventLogTranscriptAdvanceSink` are unchanged in shape; `check_projection_publication_seam.sh` passes |
| No body on the event path | The advisory's fields are unchanged and the event log is the only sink |
| No detached unbounded timers or tasks | One worker per publisher, started once, ended when the publisher is dropped (`dropping_the_publisher_ends_its_flush_worker`) |
| No idle CPU spinning | The worker sleeps on the earliest deadline and is woken by state changes; a pending advance that carries nothing new is dropped rather than left pending |
| No unbounded pending maps | `MAX_TRACKED_RUNS = 256` with oldest-first eviction; a finished run's entry is dropped at its completion flush (`a_finished_run_is_forgotten`) |
| Stale, switched, closed and cancelled route responses are discarded | The fence chain is unchanged and `a_completion_for_a_superseded_request_is_discarded` still passes |
| Only the focused tab reads eagerly | `a_tab_that_loses_the_focus_stops_reading` |
| The producer's wake is independent of unrelated run traffic | Each run's deadline is its own `last_published_at + interval`; the worker waits for the earliest one |

## 6. Failure and recovery review

- **Duplicate delivery.** An out-of-order or duplicate advisory cannot move the watermark backwards:
  the canonical storage ordinal dominates the merge, and a pending advance that carries nothing new
  is dropped rather than published twice (`an_out_of_order_advance_never_moves_the_watermark_backwards`).
- **Cancellation races.** A flush in flight and a concurrent append are serialised by the coalescing
  lock. The flush takes the pending ordinal under the lock and updates the run's last published
  ordinal there, so an append that lands during the publication finds a floor that is already the
  flushed ordinal and stays pending until the next deadline.
- **Daemon or node restart.** The producer's state is in-memory and per-publisher; a restarted
  daemon starts with an empty map, and the durable store remains the source of truth. A run whose
  advisory was lost is re-read by cursor.
- **Partial persistence failure.** Unchanged: the publication happens after the store's append
  returned, and a publication failure is logged and swallowed.
- **Stale generation or lease.** Unchanged: the route token's reconnect epoch still fences
  pre-reconnect completions.
- **Contention and resource release.** The flush worker ends when the publisher is dropped, so a
  daemon shutdown does not leave a timer behind.
- **Malformed or unauthorized input.** Unchanged: the sink drops an advance that names a run id
  that is not a run identity, and an advance whose run has no resolvable owning session is not
  published at all.
- **Bounded event and artifact behavior.** The publisher produces at most one advisory per run per
  interval, the tracked map is capped, and the TUI's page and cache bounds are unchanged.

## 7. Migration and compatibility review

- No schema migration: the durable store is unchanged.
- No protocol change: the advisory's fields, the `CoreEvent` variant and the projection event are
  unchanged, so an older client still understands the advisory.
- No configuration change: `ADVANCE_ROW_CAP` and `ADVANCE_INTERVAL` keep their values and their
  meaning; the interval is now also the quiet-run bound, which is stated in the module documentation
  and in `architecture/projection.md`.
- No compatibility shim is needed: the pass lifecycle is internal to the TUI, and the producer's
  timer is internal to the producer.
- Rollback limitation: rolling back to the pre-fix code restores the two defects (the four-page
  dead-end and the unbounded quiet-run delay). The stored data is unaffected either way.

## 8. Security review

- Authorization is unchanged: the TUI's page read is still the daemon's authorized read, and the
  advisory is still body-free, so a hint never discloses a row's content.
- The producer's timer publishes nothing but the run id, the ordinal and the stopped flag — the same
  three fields the advisory already carried.
- The sink resolves the owning session from the run's **durable task origin**, not from a
  caller-supplied session id, so an advance cannot be published under a session that does not own
  the run.
- The flush worker holds no authority of its own: it publishes through the same injected sink, and
  the sink is the daemon's canonical event log.
- Denial-of-service bounds: one worker per publisher, a capped tracked map, a capped number of
  published advisories per run per interval, and unchanged row/page/cache bounds on the TUI side.
- Redaction is unchanged: the transcript's write path still redacts at write, before anything is
  durable, and the advisory never carries a body.
- Audit behaviour is unchanged: the read path's instrumentation and the authorization matrix are
  untouched, and `check_authorization_matrix.py`, `check_audit_coverage.py` and
  `check_delegated_transcript_store.py` all pass.

## 9. Documentation and operations

- `architecture/projection.md` — new section "The producer's coalescing, and the quiet-run bound",
  stating the bound, what the timer is and is not, and its boundedness.
- `architecture/tui.md` — the catch-up paragraph now describes the pass lifecycle and the yield.
- `docs/tui.md` — the operator-facing summary says the bound is one pass, not the tab's lifetime.
- `src/agent/transcript_publisher.rs` — module documentation names the quiet-run bound, the
  worker's boundedness and the owning architecture document.
- `src/tui/commands/transcript_follow.rs` — module documentation names the pass lifecycle and
  states that the TUI still has no timer.
- `src/tui/app/state/transcript_cache.rs` — the pass-lifecycle methods carry their own
  documentation.
- Static guards: no new guard script was added. A guard that asserts the wake-up path exists would
  be a line-pattern check, which this repository deliberately distrusts (see `AGENTS.md`,
  "Never scan Rust with a regex"), and it would not be non-vacuous: a stray `sleep` in an
  unreachable function would satisfy it. The runtime tests above are the proof, and they fail on the
  pre-fix tree.
- Recovery instructions: a stuck delegated tab is no longer a state the product can reach; a
  publisher whose worker has ended is a publisher whose daemon is gone.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `watch` wake is a counter, not a "pending" signal, so a worker that wakes for an unrelated reason re-evaluates the deadlines rather than being told what changed | Bounded: the worker recomputes the earliest deadline and sleeps again; no busy loop, because a pending advance that carries nothing new is dropped | none |
| low | The producer's tracked map evicts the oldest run at the cap, so a run evicted while it still had a pending advance loses that advisory | Bounded and recoverable: the durable store remains the source of truth and a cursor read recovers the rows. Sixteen concurrent runs are far below the cap | none |
| low | The quiet-run test's wall-clock window is 25 ms against a 5 s deadline | The assertion is about *what* is published, not about how fast; the policy's "not before the interval" half is asserted deterministically with an injected clock | none |

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.** C001's two defects are corrected in production
code, both corrections are proven against a red baseline and against a real-daemon composition, and
the stop conditions were not reached: the solution is a producer-owned bounded timer plus an
event-loop pass yield — not a session-snapshot poll, not a per-append spawned task, not a global
unbounded ticker, not unauthorized pre-commit publication, not a new child session, not looser
redaction, not a bypassed principal check, not a change to the loop's or the scheduler's control, and
not a raised row or cache bound.

C002 remains open as a separate corrective milestone; its own record states its disposition.

## 12. Registry updates

- `plans/subsystems/delegated-run-live-transcript-post-closure-corrective-addendum.md` §4 — the C001
  status table row moves from `ready` to `closed`, with this record as the closure link.
- `plans/subsystems/delegated-run-live-transcript-roadmap.md` §13 — the post-closure tracking
  paragraph records C001 as closed and names this record.
- `plans/registry.md` — the corrective subsystem's row records that C001 is closed and that C002 is
  the remaining open milestone. The stale "Blocked" rows for the original M002/M003 remain
  removed by the plan author's own commits; this milestone did not rewrite them.
