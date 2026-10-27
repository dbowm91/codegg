# Delegated Live Transcript Milestone M003 — Closure Status

- **Plan**: `plans/implementation/delegated-run-live-transcript/003-tui-live-follow-and-qualification.md`
- **Roadmap**: `plans/subsystems/delegated-run-live-transcript-roadmap.md` §7 (M003)
- **Branch**: `plan/delegated-run-live-transcript`
- **Status**: closed

## 1. Executive finding

The delegated subagent tab used to be a **snapshot**: it read the transcript
when it opened and never changed again, so a parent watching a working child
saw a frozen pane. The tab is now a **live follow**.

It follows through M002's authorized page read
(`CoreRequest::AgentRunTranscriptRead`) and keeps up through M002's body-free
advisory. There is **no timer** behind it. Three things make it read:

1. the daemon publishes a body-free hint when a delegated run's recording
   advances;
2. the tab raises its desired high-water and fetches the pages the hint says
   are there;
3. a tab that is opened, focused, or reconnected catches up from the
   authoritative store regardless of whether a hint was ever seen.

What the tab renders is the **recorded transcript**, not the provider's raw
buffer: the child's own words, the calls it made and the results it got, as
the daemon durably recorded them at the moment each happened. The provider's
private reasoning is not in it and never was.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The tab renders the rows the page read fetched | `tests/tui_render/transcript_follow.inc` — `the_tab_renders_the_rows_the_page_read_fetched` | pass | The fetched rows are the live surface |
| Fetched rows win over a stale snapshot preview | `the_fetched_rows_win_over_a_stale_snapshot_preview` | pass | The preview never overwrites fresher fetched entries |
| A bounded window says that earlier entries were not loaded | `a_bounded_window_says_that_earlier_entries_were_not_loaded` | pass | The cache cap is a **window** fact, distinct from the store's truncation |
| A tab with no durable identity says live follow is unavailable | `a_run_with_no_durable_identity_says_live_follow_is_unavailable` | pass | No guessed numeric join; the tab stays the inspector it was |
| A follow that is still recording does not claim completeness | `a_follow_that_is_still_recording_does_not_claim_the_transcript_is_complete` | pass | The recording state is shown truthfully |
| A source whose own recording stopped is not shown as complete | `a_source_whose_own_recording_stopped_is_not_shown_as_complete` | pass | The store's per-run bound stays distinguishable from the viewport's |
| A scrolled-up view is anchored when new rows arrive | `a_scrolled_up_view_is_anchored_when_new_rows_arrive` | pass | The reader keeps their place |
| A view pinned to the bottom follows new rows | `a_view_pinned_to_the_bottom_follows_new_rows` | pass | The pin is explicit, not inferred from offset 0 |
| The pre-existing C003 render and viewport assertions still pass | `cargo test --test tui_render` — 1109 passed, 0 failed | pass | The frozen C003 suite is unchanged |
| Exactly one viewport owner | `src/tui/app/render.rs` — `viewport_owner()` and the single-owner dispatch are unchanged | pass | The change review still outranks the tab, and the tab still outranks the transcript |
| Every async completion is fenced | `src/tui/commands/transcript_follow.rs` — `live_route_check`, the per-cache request id, and the route token | pass | Session switch, project switch, reconnect, close/reopen and run replacement all discard the old callback |
| The event loop does not block on a read | The page read is issued in a spawned registered TUI task and returns a `TuiCommand` | pass | No read happens on the render path |

## 3. Production implementation evidence

### The per-tab cache — `src/tui/app/state/transcript_cache.rs`

| Item | Role |
|---|---|
| `TranscriptCache` | The tab's paging state: durable run id, route generation, next ordinal, high-water, completeness/recording-stopped, in-flight request id/epoch, and a bounded deque of redacted entries |
| `TranscriptFollowStatus` | The tab's honest state: unavailable older peer, denied, temporarily disconnected, recording absent, complete, truncated, partial live, catching up, read failure |
| `PageOutcome` | What a page did: appended, a duplicate, a gap, the end |
| `MAX_TRANSCRIPT_CACHE_ENTRIES` (512) | The tab's own window cap — a **window** fact, reported as "earlier entries not loaded" |
| `MAX_TRANSCRIPT_PAGE_ROWS` (64) | The rows one page read may ask for |
| `MAX_TRANSCRIPT_PAGES_PER_PASS` / `MAX_TRANSCRIPT_FETCHES_PER_PASS` (4) | The per-refresh budget, so a follow never runs away |
| `MAX_TRANSCRIPT_GAP_RECOVERIES` (3) | The bounded gap recovery, so a repeated gap cannot loop |

The cache is **separate** from `SubagentRow`'s preview. That separation is the
point: the preview is the projection's boundary read and the cache is what the
authorized page read fetched. A snapshot that arrives late must not overwrite
fresher fetched entries, and the render path takes the fetched rows first.

### The tab state — `src/tui/app/state/subagent_tabs.rs`

`SubagentTabState` gained three fields, each with a distinct job:

- `transcript: Option<TranscriptCache>` — the paging state. `None` when the
  tab has no durable run identity or live follow is unavailable.
- `follow_bottom: bool` — **the only thing that decides** whether newly
  recorded rows pull the view down. It is set by an explicit jump to the
  bottom and cleared by an explicit scroll upward. It is deliberately not
  derived from the offset: a fresh tab starts at the top of a long
  transcript, and treating "offset 0" as "pinned" would hide every new row
  below the fold.
- `scroll_max: Option<u16>` — the last extent the renderer computed. Only
  the renderer knows how tall the content is, so it writes this back; the
  tab uses it to turn "pinned to the bottom" back into a real offset when
  the user scrolls up from there.

`follow_unavailable: Option<String>` names **why** the tab cannot follow, so
the pane says so rather than sitting empty.

### The follow command — `src/tui/commands/transcript_follow.rs`

| Function | Role |
|---|---|
| `ensure_cache` | Binds a cache to a tab's durable run id, or records why it cannot |
| `start_follow` | Starts a follow for an open tab |
| `issue_page_read` | Issues the bounded page read, under the per-tab and across-tabs request limits |
| `apply_page` | Merges a page by `(run_id, ordinal)`, ignores duplicates and stale envelopes, recovers a gap from the authoritative cursor, and applies the request-id and route fences |
| `status_for_error` | Classifies a failure into the honest states (denied, disconnected, unavailable, read failure) |
| `on_snapshot_watermarks` | The advisory handler: raises the desired high-water for the **open and active** tab and coalesces with an in-flight fetch |
| `on_tab_focused` | The catch-up on focus, for a background tab that remembered only bounded dirty state |
| `retry` | The explicit retry affordance |
| `outstanding_reads` | The fair total limit across open tabs |

A **background** tab remembers only bounded dirty state and fetches on focus:
it generates no recurrent traffic. A newly opened tab catches up from the
authoritative store even if no hint was ever seen.

### The render — `src/tui/app/render.rs`

- The scroll extent is computed from the **wrapped** height
  (`Paragraph::line_count(area.width)`), not from the number of logical
  lines. A body that wraps onto three visual rows is three rows on screen,
  and an extent computed from logical lines would let the view stop short of
  the real end and would claim a line count the screen never showed.
- Whether the view was at the bottom is decided **before** this frame's
  content grew, so a view that was at the bottom follows the new rows and a
  view that was scrolled up stays anchored.
- The renderer writes `scroll`, `scroll_max` and `follow_bottom` back onto
  the tab, because it is the only place that knows the content height.
- The live-follow state line comes **first**, before the transcript: a user
  who cannot see that cannot tell a paused follow from a finished run, and
  an empty pane must never read as "the subagent did nothing".

## 4. Verification executed

### Commands run

```bash
cargo test --test tui_render -- transcript_follow
cargo test --test tui_render
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
```

### Results

| Command | Result |
|---|---|
| `cargo test --test tui_render -- transcript_follow` | 8 passed, 0 failed |
| `cargo test --test tui_render` | 1109 passed, 0 failed — the frozen C003 suite included |
| `scripts/check_tui_transport_authority.py` (+ `--self-test`) | passed |
| `scripts/check_command_catalog_ownership.py` | passed |
| `scripts/check_frontend_action_set.py` | passed |
| Every guard in `scripts/verify.sh quick`'s `run_guards()` (82 guards) | 0 failures |

The full workspace sweep (`cargo nextest run --workspace --locked --profile ci --no-fail-fast`)
runs 16,600 tests: **16,569 passed**, 30 failed, 1 timed out, 9 skipped. Every
one of the 31 non-passing tests was checked against the **baseline** (this
branch's `HEAD` with this line's changes stashed): **29 of the 30 failures
reproduce identically there**, and the thirtieth passes on its own in this
tree. None of them is in this milestone's surface. The detail is recorded in
M001's closure record §4.

This milestone's own surface is green: the whole `tui_render` family —
including the frozen C003 layout, viewport and keypress assertions — is
**1109 passed, 0 failed**, and the new `transcript_follow` member is
**8 passed, 0 failed**.

## 5. Invariant review

| Invariant | How it holds |
|---|---|
| Exactly one viewport owner | `viewport_owner()` and the single-owner dispatch are unchanged; the change review still outranks the tab |
| One spinner, one activity phase | The follow adds no spinner; it reuses the existing surfaces |
| Never synthesize a row the projection did not vouch for | The tab renders fetched rows and the projection's preview. It never invents an entry, and it never renders a fabricated "the child did nothing" |
| The TUI holds no durable authority | The tab caches redacted entries for display only; the canonical cursor and the canonical content come from the daemon's authorized read |
| No unbounded work per frame | The per-refresh budget, the per-tab request limit and the across-tabs limit bound the work; the render path does no I/O |
| No polling timer | There is no timer. The follow is driven by the advisory, by focus, and by open |
| The transport discriminator is respected | Answers and reads go through the existing CoreClient path; `App::is_ws_attach_transport()` remains the transport discriminator, not a staleness fence |

## 6. Failure and recovery review

| Scenario | Behaviour |
|---|---|
| An in-flight page fetch vs a tab close | The stale result is dropped by the request-id and route fences |
| A focus shift | Only the focused tab eagerly fetches; another tab's hint marks it dirty |
| A session or project switch | The old route is invalidated and the sensitive cache is dropped |
| A daemon reconnect | The client re-subscribes and reconciles its cursor from the authoritative page, never from the last hint alone |
| A dropped or out-of-order hint | The next page read recovers from the authoritative cursor; the watermark is only a trigger |
| A gap in the ordinals | The gap is recovered from the authoritative cursor reader, bounded by `MAX_TRANSCRIPT_GAP_RECOVERIES` |
| The child fails, cancels, or is restarted | The final flush is shown and the terminal status is correct |
| A tool call awaiting a result | It remains visibly pending; nothing marks it successful |
| A long transcript | The cache cap is stated honestly ("earlier entries not loaded"), the store's own truncation is preserved, and the scroll stays stable |

## 7. Migration and compatibility review

The TUI's protocol use is additive. Against a daemon that does not serve
`CoreRequest::AgentRunTranscriptRead`, the tab keeps the inspector it was and
says that live follow is unavailable — it does not attempt a guessed join and
does not show an empty pane. Against a daemon that serves the read but never
publishes the advisory, the tab still catches up on open, on focus and on
reconnect, because the catch-up reads the authoritative store rather than
waiting for a hint.

No new session identity, no new durable frontend store, and no new execution
surface were introduced.

## 8. Security review

| Property | Where it holds |
|---|---|
| The TUI cannot turn an old unauthorized snapshot into a new read | The page read is authorized by the daemon on every read; the TUI's cache is display state, not authority |
| No cache across project/session owner changes | The route fence drops the cache when the project or session changes |
| Redacted bodies are not logged unnecessarily | The follow path renders bodies; it does not log them |
| An observer frontend still relies on daemon authorization | The read is a daemon-authorized `CoreRequest`; the TUI holds no authority of its own |

## 9. Documentation and operations

| Document | Change |
|---|---|
| `docs/tui.md` | New "The live follow" section replacing the boundary-read description |
| `architecture/tui.md` | "M003 makes the tab a live follow, and the follow is not a poll" — the three mechanisms, the states, and the scroll rules |
| `.opencode/skills/tui/SKILL.md` | The live-follow contract next to the projection-snapshot and viewport-ownership rules |

The docs now distinguish the **live recorded transcript** from the provider's
raw buffer, state the capture granularity, and state the explicit lack of
hidden reasoning and its bounds.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The follow is driven by the advisory, so a daemon that publishes no advisory makes a background tab follow only on focus | A background tab is stale until focused | Acceptable: the catch-up reads the authoritative store, and the active tab is driven by the advisory |
| low | The end-to-end qualification ran against a paused-tool fixture rather than a live provider | No claim about a live model's transcript fidelity | None: the milestone explicitly does not require a live model |

No finding blocks the milestone.

## 11. Roadmap disposition

M003 is closed. The delegated live transcript line — M001, M002 and M003 — is
complete.

## 12. Registry updates

`plans/registry.md` records M003 as closed and the subsystem roadmap as
closed.
