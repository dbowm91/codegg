# Delegated Live Transcript Post-Closure C001 — Forward Progress and Advisory Liveness

Status: ready for handoff
Primary class: invariant
Repository review baseline: plan/delegated-run-live-transcript at 21dca074b09eb00c34527ff13067352d0641897d (2026-10-26). Revalidate HEAD before coding.
Source roadmap: plans/subsystems/delegated-run-live-transcript-post-closure-corrective-addendum.md §4 / C001.
Long-term: plans/000-long-term-specification.md (bounded, reliably observable delegated execution; daemon publishes projections), plans/001-terminology-and-domain-model.md.
Accepted ADRs: ADR-0051 durable, redacted run transcript; ADR-0043 session-owned subagent lifecycle; ADR-0041 parent-mediated steering. Closed dependencies: delegated-run-live-transcript M001–M003 on feature branch. Integration successor: corrective C002, blocked until this plan closes.

## 1. Objective

A TUI actively following a durable subagent must make bounded *forward progress* after every authorized cursor page, even if the transcript has more than four pages; a semantic transcript advance coalesced while the child is quiet must reach its subscribed owning session in bounded time, without an extra store append or run completion to trigger it.

Close the two regressions with baseline-failing tests and minimal producer/TUI scheduling changes. This milestone owns *liveness correctness*, not broad CI lint cleanup.

## 2. Why ready

M001–M003 landed: in-loop capture in src/agent/transcript_capture.rs, coalescer and EventLog sink in src/agent/transcript_publisher.rs, bounded authorized CoreRequest::AgentRunTranscriptRead, TUI cache in src/tui/app/state/transcript_cache.rs, follow orchestration in src/tui/commands/transcript_follow.rs. The observed defects concern when to resume already-authorized work, not new principal, store, protocol or scheduler authority. No ADR is necessary if fixes remain within the existing owner boundaries.

## 3. Measured implementation evidence and failing scenarios

**L1 — four-page catch-up dead-end.** start_follow returns at cache.pages_in_pass >= MAX_TRANSCRIPT_PAGES_PER_PASS (4). apply_page increments pages_in_pass and recursively starts another fetch only if has_more and the counter is below four. With has_more after fourth page, the counter remains four and cache.dirty remains true; it resets to 0 only when !dirty. on_tab_focused and on_snapshot_watermarks call start_follow, which hits the same cap. retry() is limited to is_retryable error statuses; ordinary CatchingUp is not retryable. A cursor stuck at ordinal 256 out of, for example, 320 remains stuck despite the daemon possessing later rows. This is a concrete path in source, not merely a missing test.

**L2 — interval is not a quiet-run deadline.** CoalescingTranscriptPublisher::publish_advance invokes should_publish only upon an append. should_publish tests elapsed >= ADVANCE_INTERVAL (500ms) but schedules no wake. After first immediately published advance and 1–7 quick appended rows, the pending watermark is retained until another append, recording-stopped event or worker final flush. If the child then waits on a tool, no hint reflects those rows. A focused tab that has already read the first high-water has no reason to read again. This does not meet the intended near-live view. The first-publish and terminal-flush tests currently mask this gap.

Verification reference: M003 closure's "no polling timer" is a TUI restriction. A bounded *daemon publisher* timer/flush is not a TUI poll, but complexity must be proportionate. Another valid bounded solution is a well-defined semantic idle-boundary flush with proof for tool/provider waits. Do not label a merely conditional on-next-append check a bounded-time guarantee.

## 4. Invariants not to regress

- At most one page request per tab; fair aggregate request bound across tabs; the per-pass limit of 4 genuinely caps each processing cycle, not the lifetime of that tab. No synchronous read or unbounded drain on the TUI render path.
- Cache next_ordinal is *only* the store's authoritative cursor; no event-derived cursor advancement or imagined missing entries. Run id, parent session and route epoch checks stay intact.
- Retain the run's own 256KiB store bound, typed page row/byte caps, 512-entry cache/window cap, and distinguish RecordingStopped, Absent, Complete and CatchingUp. Hidden tabs are not polled.
- Advisory contains only (owner session, run id, ordinal, recording_stopped); never body/text/args/reasoning/credentials. Publishing remains strictly after commit through EventLog, independent of opened tabs; no new direct projection publisher.
- Do not introduce detached unbounded timers/tasks, idle CPU spinning, unnecessary full-session snapshot refresh, or change execution control/permission policy.
- Progress must not starve another visible tab; stale/switch/close/cancelled route responses must still be discarded.

## 5. Scope

In: producer pending-advance liveness and bounded wake/flush; TUI catch-up cycle scheduling; finite cursor pagination, fair read admission, cancellation/shutdown cleanup; explicit tests at real store boundaries, docs corrected where the 500ms claim is stated.

Out: full context or provider reasoning, changing transcript persistence, wider model/tool semantics, human steering, changing status DTO or owner authorization, desktop follow feature, generic polling framework, unrelated Clippy errors (C002).

## 6. Required production changes

Core/domain and storage: unchanged. Continue append via AgentRunTranscriptStore, cursor from read only. No new durable table or migration.

Protocol/DTO: no schema/variant changes expected. Keep post-commit advisory compatible with existing client.

Runtime/producer:
- Establish an explicit upper bound between *committed append* and *published high-water* for a quiet run. A low-overhead bounded daemon-owned coalescer flush or a proven flush at semantic idle boundaries may be used, provided the test reproduces 'appended 2–4 entries, blocked indefinitely, advance delivered within bound'. Do not rely on more appends, source truncation or final run completion.
- If timer-based, choose a *single bounded producer-owned mechanism* with cancellation/shutdown semantics and a cap on retained pending run ids; no per-append tokio::spawn or per-TUI tab timers. A timed producer flush is not TUI read polling. Wake independent of unrelated run traffic. Avoid holding the coalescing Mutex while awaiting EventLog. Only highest committed monotonic ordinal published; lost publication is recoverable by cursor read.
- Guard race between an in-flight flush and a concurrent append, between recording-stopped and flush, and around daemon drop/restart. Never publish a watermark lower than the already published one. If the publisher cannot guarantee scheduled flush without significant new infrastructure, prefer an explicitly justified bounded semantic direct-event policy and measure the event count.
- Bounded pending maps cannot accumulate completed run entries forever; inspect/qualify cleanup. Do not invent cross-session join or authority.

Runtime/TUI:
- Define a "pass" lifecycle: a pass consumes at most four pages; when the store still has_more, it yields and schedules another bounded pass through the existing command/event loop (not recursive unbounded reads or a permanent four-page lockout). Progress must be monotonic until caught up, paused by a real backpressure/permission failure or explicitly stopped; a hidden or closed tab must not continue eager fetching.
- Keep the fair global outstanding request limit (4) and per-tab in-flight limit (1). If another tab holds the budget, make sure a later completion/focus is a wake for the waiting active tab rather than an indefinite no-op.
- For a large snapshot or run already finished, catch-up should advance across 5+ pages without needing additional daemon advisories, another tab focus, or a retry key.
- Preserve scroll anchoring/pin-to-bottom, cache eviction honesty, route epoch fencing and terminal state. A bounded yield is a scheduling decision, not a terminal 'read failed' status.

Frontend/operator: display CatchingUp until the final page; never say Complete when has_more. No new controls unless required to expose a persistent external read failure.

Security: all page content remains from the daemon-authorized read; no raw provider event body on the stream. Failing listener cannot turn stale route into another session's displayed content.

Documentation/guards: precise lifecycle comments in transcript_publisher.rs, transcript_follow.rs and transcript_cache.rs; architecture/projection.md and docs/tui.md only if behavior claims change. Add guard assertion/self-test if the repository's static guard pattern can non-vacuously detect removed wake-up logic; runtime tests are mandatory regardless.

## 7. Ordered work packages

### A — Reproduce both liveness failures
Implement deterministic tests *before the fixes*, not sleeps as weak proxies:
- Fake CoreClient with 6 pages of 64 rows (has_more true on page four), instrumented request cursors, TUI command pump; assert old tree stalls after four and stays stalled after focus/advisory.
- Coalescer fake sink: first row delivered, 2–4 rows appended inside short interval, no further append, clock advances beyond interval while simulated child remains alive; assert old tree has no advisory. Use paused Tokio time or injected clock plus controlled wake; no wallclock/flaky duration inference.
Record baseline red and preserve it in closure evidence.

### B — Correct producer wake/flush
Add bounded pending high-water wake/flush with no unbounded tasks. Assert one advance carrying latest ordinal by deadline even if idle; no duplicate/stale event, no body, and exactly one source-run scope. Test terminate/flush, cancel and daemon drop/restart separately. Measure publish count for 16 concurrent runs and prove cap. Update known stale comments that describe interval as guaranteed.

### C — Correct TUI multi-pass progress
Implement fair yield and re-entry after each four-page pass. Assert cursors 0,64,128,192,256,... until has_more false with no new hint, no repeated cursor 256, no hot-loop and bounded outstanding reads. Repeat with two active tabs/rapid focus and with route switched halfway; ensure no stale delivery. Preserve user's scrolled-up position and cache truncation marker.

### D — Compose runtime proof
A real SQLite-backed run blocked in a tool emits call/result checkpoints and the daemon publishes an advance while the child is still blocked. A CoreClient issues page reads and the live TUI receives/redraws from that path. Inject a long transcript above 4 pages before opening tab; prove full catch-up. Disconnect and reconnect with pending advances; prove final tail and no foreign-run leak. Retain security tests and M001–M003 focused suites.

## 8. Failure, cancellation, restart and contention

Lost hint -> explicit open/focus/reconnect page read restores, without guessing. Publisher stopped between commit and signal -> stored row survives and reader catches up on reconnect; normal quiet run must eventually emit within the chosen bound. Close/reopen tab -> pending command fenced, read tasks don't leak or mutate reopened tab. Callback after session switch -> discard. Cancellation while a tool call is pending -> no fabricated result; final flushed recorded prefix remains. Store cap hit -> RecordingStopped event remains prompt. 16 children -> one bounded event queue and a fair TUI, no unbounded per-run task or DB read explosion.

## 9. Compatibility and migration

No schema or protocol version bump expected. An old daemon's lack of advisory/read remains explicitly unsupported in a new TUI, with C003 snapshot fallback; an older TUI receives no body from advisory and retains snapshot behavior. New producer policy only changes advisory liveness, not content or security.

## 10. Required tests

Focused: 5+/6-page >4-pass cursor recovery; stale/focus/terminal without new hint; source has_more semantics; fairness/backpressure; typed page bound refusal; cache eviction/scroll. Producer: quiet-run timed/coalesced advance, duplicate/out-of-order ordinal, recording-stopped immediate flush, run completion flush, shutdown/pending cleanup, 16-run publish cap. Integration: actual daemon authorization and event stream, child block via deterministic FIFO/barrier, persist before termination, live TUI update and reconnect; session A/B negative and observer denied. Mutation tests: remove the pass re-arm, disable producer wake, prove failure. No provider-backed fidelity claim without an actual provider run.

## 11. Required verification commands (execute and report; not yet run)

- cargo test --test tui_render -- transcript_follow
- cargo test --test tui_render -- delegated_transcript
- cargo test --test agent_loop -- delegated_transcript
- cargo test --test core_daemon -- delegated_transcript
- cargo test --test projection_replay -- delegated_transcript
- cargo test --lib -- transcript_publisher
- python3 scripts/check_delegated_transcript_store.py --self-test
- bash scripts/check_projection_publication_seam.sh --self-test
- bash scripts/check_projection_disclosure.sh --self-test
- python3 scripts/check_tui_transport_authority.py --self-test
- python3 scripts/check_projection_transport_lifecycle.py
- cargo fmt --all -- --check
- bash scripts/verify.sh quick

The repository has known baseline Clippy failures: C001 must report, not mask, them; C002 owns that gate. Adapt actual test target/CLI only after verifying the source tree and record the actual commands.

## 12. Documentation updates

architecture/projection.md, architecture/tui.md, docs/tui.md, module doc comments where they assert timing or catch-up completion. Historical closures remain unmodified; new closure owns new findings.

## 13. Acceptance criteria

1. A >4-page authorized transcript catches up to has_more=false in one open active tab via bounded yielding, with no new daemon hint, manual retry or refocus. Each pass remains <=4 pages and each tab <=1 outstanding request.
2. After an initial event, 1–7 new committed rows followed by a quiet blocked child lead to an event for the highest committed ordinal within a documented bounded period *without further appends*; no body appears on the event path.
3. Mixed 16-run concurrency does not create an unbounded producer/task state or runaway TUI fetches; source is authoritative when events drop.
4. Route/session switches, reconnect and close/reopen never display stale foreign rows; original redaction/read authorization and scroll/eviction behavior are unchanged.
5. Deterministic tests fail on baseline code for both defects, pass with corrections, and include a real-daemon composition receipt.

## 14. Stop conditions

Stop if the only solution is polling session snapshots, a per-append spawned task, a global unbounded ticker, unauthorized pre-commit publication, a new child session, loosening path/secret redaction, bypassing principal checks, changing loop/scheduler control or increasing the row/cache bounds. Report and propose a focused ADR/plan correction rather than hiding an inability to make progress.

## 15. Closure evidence

Record source commit, baseline-failing proof, corrected cursor trace and high-water timing, 16-run publication/catch-up count, real-daemon + TUI fixture and route-negative matrix, focused/guard commands (pass/fail), and all known limits. Update this addendum and plans/registry.md only when evidence is accepted.

## 16. Handoff notes

Do not merge PR #203 until C002's qualification gate. C001 should use separate worktree from unrelated TUI work. The producer fix and TUI follow fix may be implemented separately only with a single integration owner, because they jointly determine whether the tab is actually live. Avoid changing the historical M003 closure to disguise a new issue.
