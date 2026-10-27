# Delegated Run Live Transcript M003 — TUI Live Follow and Qualification

Status: blocked — hard dependency M002 complete with typed authorized page and change signal
Primary class: capability
Repository review baseline: main 0be3e0a53ba97ee51f6d64aba2235bdef92452e7, 2026-10-26; revalidate after M001/M002.
Source roadmap: plans/subsystems/delegated-run-live-transcript-roadmap.md §7 M003.
Long-term: plans/000-long-term-specification.md (frontends render projections, TUI, privacy and recovery); plans/001-terminology-and-domain-model.md.
Accepted ADRs: ADR-0051 (durable redacted transcript), ADR-0043 (session-owned child lifecycle), ADR-0041 (steer parent, not child), ADR-0053 (daemon-published frontend surfaces). Historical closure: plans/closure/delegated-run-observability-and-recovery-corrective/003-status.md.

## 1. Objective

A durable subagent's open TUI tab follows its recorded conversation as it executes, displaying newly committed assistant/tool call/tool result rows without tab reopen. Maintain safe scroll behavior and recover from event lag or disconnect by fetching bounded new pages using M002's run+session cursor API. The existing run inspector, task overlay, session navigation and tab cycling remain intact.

## 2. Readiness and dependencies

M002 is a **hard** dependency, not currently closed. Its exact-owner CoreRequest/CoreResponse, body-free transcript-advanced hint and replay/resync semantics must be settled before final TUI wiring. The current TUI already has tabs, viewport ownership, scrolling, tool formatting, transcript state labels and an explicit snapshot refresh on tab open (C003). The UI portion can be unit-tested with a fake CoreClient once M002's interface is stable, but cannot close on fake events alone.

## 3. Current implementation evidence

- src/tui/app/mod.rs::open_subagent_tab calls refresh_delegated_transcript, and src/tui/commands/primary_projection.rs::refresh_session_snapshot replaces a session subscription. No automatic follow after initial read.
- src/tui/app/state/subagents.rs::SubagentRow already carries Option<AgentRunTranscriptProjection> with None (field not carried) distinct from Absent (run recorded no transcript).
- src/tui/app/state/subagent_tabs.rs stores last known row, attached/detached state, scroll and a capped 16-tab set; its identity is derived from projection child id/task id. Durable transcript reads instead require canonical run id, not incidental task id or display name.
- src/tui/app/state/session_work.rs joins session-owned child nodes to durable AgentRunSummary by canonical run id, avoiding double-counting. On a matching run summary it copies transcript only when present.
- src/tui/app/render.rs::render_subagent_view renders metadata plus delegated_transcript_lines; per-entry display limit is 4 KiB. PageUp/PageDown already scroll the active tab and viewport owner prevents main conversation bleed-through.
- docs/tui.md explicitly says the tab reads at open, not live. Older daemon/older run fallback is a run inspector with unavailable/absent provenance. C003 end-to-end tests exist under tests/tui_render/delegated_transcript.inc and tests/projection_replay/delegated_transcript.inc.

## 4. Invariants

- Only daemon-authorized, redacted, bounded transcript pages enter TUI state. No client-side SQLite, direct tool log or parent message scraping as a fallback.
- A tab can display entries for exactly its captured (project, session, durable run id). A selected child without run id must stay a metadata inspector; no join by name, numeric task id, or a guessed identifier.
- The UI never treats a received change hint as a transcript body or as proof of successful/cancelled work. Run status comes from authoritative lifecycle projection.
- No user-to-subagent send affordance; ADR-0041 steering still targets the parent, and closing a tab never interrupts the child.
- Existing Main tab, Shift+N/P/X Normal-only behavior, Ctrl+T in both modes, /sessions and sidebar activation, modal priority and viewport opacity are unchanged.
- A truncated preview/page is not reported as complete; no silently missing messages, duplicates, or incorrect auto-scroll.

## 5. Scope

In: per-tab cursor/lineage cache; open-tab initial page and incremental follow, queue coalescing/throttling, stale-response fencing, scroll handling, explicit loading/catching-up/truncated/error states, reconnect recover, integration and visual regression tests, docs.

Out: daemon protocol and capture (M001/M002), a new child session or tab identity unrelated to the run, more than 16 tabs, full context/hidden reasoning, raw provider token streaming, child interaction/cancel, desktop/ACP integration, eager background tail reads for all child runs, automated polling when no tab is open.

## 6. Required production changes

Core/domain/storage: none; consume only M002 typed CoreClient calls.

Protocol: consume M002's session-scoped transcript-advanced hints through the existing projection event loop with canonical run id and committed high-water. Initial projection preview remains valid for older daemons. Page state must have a genuine next ordinal and has_more; do not infer completeness from empty display lines or from number of rows painted.

Frontend state:
- Represent a tab's transcript cache with durable run identity, project/session route generation, next ordinal, high-water, completeness/recording-stopped state, in-flight request id/epoch, and a bounded deque of redacted entries. Keep last-known inspector metadata separate from transcript paging state. If implementation reuses SubagentRow's preview, guard against overwriting fresher fetched entries with an older snapshot.
- On tab open, derive canonical run id from the already authorized session work projection, use any recorded preview consistently, and initiate bounded cursor fetch. If the snapshot lacks a run id or peer lacks M002, present the existing inspector and explicitly label the live-follow capability unavailable; do not attempt a guessed numeric join.
- On body-free hint matching an **open and active** tab, raise the desired high-water and coalesce with an existing in-flight fetch. Fetch pages while has_more and within a strict per-refresh budget, with a bounded outstanding request per tab and a fair total limit across open tabs. For non-active/background tabs, remember only bounded dirty state, fetch on focus, and do not generate recurrent traffic for hidden tabs. A newly opened tab must catch up from authoritative DB even if no hint was observed.
- Merge by (run_id, ordinal), keep order stable, ignore duplicate/stale envelopes and recover a gap from the authoritative cursor reader. Terminal hint/status always triggers final catch-up when possible. Ensure source truncation remains distinguishable from viewport/display truncation and page-window retention. Define explicit behavior after tab cache cap (e.g. keep bounded tail plus 'earlier entries not loaded' marker; scrolling cannot claim complete historical transcript when pages were evicted).
- Keep scroll position anchored when a user has scrolled upward; only auto-follow the bottom if previously pinned to bottom. PageUp/PageDown and viewport scroll target subagent view, not hidden parent transcript. When new body wraps to more visual lines, clamp after wrapping, do not jump arbitrarily or claim line count from raw entry count.
- Route all async completions through existing TuiCommand generation/request/route fencing: session switch, project switch, reconnection, close/reopen, task-id reuse and run replacement must discard old callbacks. No stale view can reveal another project's/session's transcript after a tab switch.
- Error/loading states: distinguish unavailable older peer, denied, temporarily disconnected, recording absent, complete, truncated, partial live, catching up, and read failure. Offer explicit retry/reopen if live follow breaks; never show an empty pane as 'subagent did nothing'.

Runtime/concurrency: event-loop integration must not block UI redraw on CoreClient reads. Concurrent 8–16 children cannot cause O(children × tokens) snapshot replacement or repaint; no polling timer for inactive sessions. An open tab with queued updates eventually catches up without unbounded work per frame. Unsubscribe/disconnect tears down cursor tasks without cancelling the run.

Security: do not log redacted bodies unnecessarily or cache across project/session owner changes. An observer frontend must still rely on daemon authorization, and the TUI cannot turn an old unauthorized snapshot into a new read after permission change.

Documentation/guards: revise docs/tui.md and architecture/tui.md, relevant .opencode/skills/tui/SKILL.md, plus async transport/route ownership guards. Explain live recorded transcript versus raw provider buffer, real-time capture granularity, explicit lack of hidden reasoning and its bounds. Do not revive obsolete docs about a purely status-only run inspector.

## 7. Ordered work packages

A. Test-first baseline: construct a fake M002 page provider and stream to verify the current code does not live-update after tab open. Record one failing TUI test against old behavior. Model user scroll pinned at bottom versus scrolled upward and error/unavailable states. Freeze existing C003 render tests.

B. Cursor state and update application: implement bounded per-tab cache, matching/high-water and async identity fence. Convert initial tab read from whole session subscription replacement to typed incremental read with compatible old-daemon fallback. Preserve inspector metadata and canonical identity joins.

C. Event and lifecycle wiring: listen to transcript-advanced hints, coalesce/limit page fetches, catch up on focus, terminal and reconnect. Add gap/duplicate/out-of-order handling. Ensure stale result after close/switch is discarded and modal priority is unchanged.

D. Rendering and ergonomics: append rendered rows without replacing the entire parent transcript, preserve reading scroll, show recording/completeness/catch-up states truthfully, keep window/byte cap obvious, and add retry path with disabled reasons.

E. End-to-end qualification: real daemon + SQLite + CoreClient + TUI under a paused tool fixture; two sessions/clients and 2+ children; produce a screenshot/text-buffer assertion of output before child completes, then reconnect, fail/cancel/restart and assess. Re-run all C003 layout/viewport/keypress assertions and static guards.

## 8. Failure, cancellation, restart and contention

In-flight page fetch vs tab close: stale result dropped. Focus shift: only focused tab eagerly fetches; another tab's hint marks it dirty. Session/project switch: invalidate old session route and clear sensitive caches. Daemon reconnect: resubscribe normally and reconcile cursor with run id/high-water from authoritative page, not last event sequence alone; never resume from cached high-water without verifying it belongs to the same run. Lag/backpressure: coalesced dirty watermark and pages recover, with an explicit UI catching-up state. Daemon restart after append: committed content visible after reconnect. Child fails/cancels: final flush shown and terminal status correct. A tool call awaiting a result remains visibly pending, not marked success. Long transcript: cache limit/earlier entries status honest; source store bound/truncation preserved; scroll stable.

## 9. Compatibility and migration

No storage migration. New TUI with old daemon (or old run with no transcript): existing C003 inspector and snapshot-on-open still work, with explicit reason that live follow is unsupported/not recorded. Old TUI with new daemon keeps current behavior. Version/feature negotiation must use actual protocol rules; don't catch and silently ignore errors as 'complete transcript'. No rewrite of historical messages or pre-milestone closure records.

## 10. Required tests

Unit/TUI: open and visibly append after hint; scroll-follow when pinned; scroll holds position when reading older lines; ordinal dedupe/gap; cache bounds and truncation notice; open two child tabs then route hint to only matching run; tab closed while page pending; stale project/session/reconnect epoch/attempt; legacy no-run-id and older daemon fallback; case of missing read result distinct from Absent.

Integration: pause a real durable child after call commitment; within same run/tab the TUI paints call while child remains blocked; release it, assert actual tool result and final model message appear *without reopen*. Inject failure and cancel at tool boundary; assert terminal data and status without successful result fabrication. Reconnect the real client after a missed hint, then assert cursor recovery. Multiple clients/sessions: only exact authorized session sees text and no mix when two children share agent display name. Visible 'earlier entries not loaded' notice after bounded cache eviction.

Visual regressions: test viewport owner (main transcript never ghosts), strip opaque/right width, main/child keys in Insert/Normal, overlay/sidebar/sessions activation, PageUp/PageDown and long wrapped Unicode entries. Performance: count CoreRequests in fan-out fixture; no recurring background reads when tabs closed/inactive; bounded requests per refresh and no unbounded memory.

## 11. Required verification commands

- cargo test -p codegg --locked --test tui_render -- delegated_transcript
- cargo test -p codegg --locked --test tui_main_buffer
- cargo test -p codegg --locked --test projection_replay -- delegated_transcript
- cargo test -p codegg --locked --lib -- tui
- cargo test -p codegg --locked --test agent_loop -- delegated_transcript
- python3 scripts/check_tui_transport_authority.py
- python3 scripts/check_tui_subagent_name_pool_isolation.py
- python3 scripts/check_projection_transport_lifecycle.py
- python3 scripts/check_projection_transport_isolation.py
- bash scripts/check_projection_disclosure.sh --self-test
- python3 scripts/check_websocket_bounds.py --self-test
- cargo fmt --all -- --check
- cargo clippy --workspace --all-targets --locked -- -D warnings
- bash scripts/verify.sh quick

Run the actual repo test target names if they have moved; do not present commands above as completed.

## 12. Documentation

docs/tui.md, architecture/tui.md, .opencode/skills/tui/SKILL.md, relevant module doc comments and help copy. Update exact stated gestures only after checking runtime key dispatch. Source/closure docs from closed predecessor milestones stay historical.

## 13. Acceptance criteria

1. A selected running durable subagent visibly acquires model/tool call/tool result entries in its TUI buffer before that run terminates, with no manual tab reopen.
2. The transcript is a read-only record with honest absent/truncated/partial statuses; no raw reasoning, invented progress or synthetic tool outcomes.
3. User scrolling upward stays anchored as entries arrive; scroll pinned to bottom follows new entries. Main conversation never paints underneath the child view.
4. Lagged/reconnecting clients recover from cursor read and stale responses cannot mix two runs/sessions.
5. A 16-child fan-out keeps read count, bytes and memory bounded; closing a tab stops associated live-fetch activity but not the child.
6. Existing C003 behavior, session controls, redaction and authorization guards remain green on a real daemon fixture.

## 14. Stop conditions

Stop if M002 is not closed or does not prove exact-owner auth and cursor semantics; if only a TUI-side timer could simulate live capture; if a run without an authorized id must be joined by name/task id; if the only implementable UI requires unbounded memory, hidden provider reasoning, or a new independent session. Record the blocker instead of weakening correctness.

## 15. Closure evidence

Commits/PR, baseline-failing and repaired tests, screen/text-buffer proof of *pre-completion* activity, fake versus real-daemon evidence explicitly distinguished, scroll and route-fence matrix, backpressure request counts, cross-session denial, reconnect/restart tests, legacy compatibility, static-guard self-tests, exact-head quick/clippy/CI results, outstanding limitations, updated roadmap/registry.

## 16. Handoff notes

M001/M002 must close first. Prefer one TUI owner. Avoid reopening deprecated keyboard mappings or rerouting main conversation event stream. Coordinate with other TUI changes through workspace/worktree isolation, use test fixtures rather than production provider keys, and preserve unrelated user changes.
