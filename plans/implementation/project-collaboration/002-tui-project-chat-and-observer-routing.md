# Project Collaboration Milestone 002 — TUI Project Chat and Observer Input Routing

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/project-collaboration-roadmap.md#M002--TUI-project-chat-and-observer-input-routing`

Long-term requirements: `plans/000-long-term-specification.md#15-read-only-session-observation`, `#21-project-communication`, `#25-tui-target-behavior`; roadmap Phase 12.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Add project-scoped chat UI/state to the TUI and route insert-mode text to project chat while observing another session, without granting observer session-control authority.

## 2. Why this milestone is ready

Blocked on collaboration M001. Presence/observation M003 is a transitive prerequisite and supplies stable observer/focus semantics; multi-project TUI is closed.

## 3. Current implementation evidence

Project tabs, session/activity views and command/focus infrastructure exist. Target behavior reserves project chat panel/tab and observer insert input, but no canonical chat state currently exists.

## 4. Invariants that must not regress

TUI renders daemon chat state and owns no durable messages; chat routes by ProjectId/ChannelId; observer typing cannot become turn steering; unauthorized/stale project data clears; large references remain handles; no unbounded message history in memory.

## 5. Scope

In: chat reducer/cache with bounded window, panel/tab, send/reply/mention/reference/edit/redact/read actions supported by M001, composing indicator, paging/resync, key/focus behavior, observer input routing. Out: structured task actions (M003), membership admin, bridge UI.

## 6. Required production changes

Frontend: project-scoped chat state/reducer, bounded message virtualization/history, composer/focus, unread/read markers and reference rendering. Protocol: consume M001 only. Runtime: existing async command/event architecture, no task-per-message. Security: never infer permission client-side; daemon denials render cleanly. Docs: keys/help.

## 7. Ordered work packages

A — TUI chat state/reducer/cursor paging per project/channel.

B — panel/tab message/reference rendering and bounded composer.

C — send/reply/mention/edit/redact/read/composing commands through existing async command pipeline.

D — observer-mode insert routing and hard regression that no turn steering/control request is emitted.

E — multi-project/reconnect/lag/retention/focus tests and docs.

## 8. Failure, cancellation, restart, and contention semantics

Failed send retains editable draft with typed error but does not fabricate message. Reconnect resumes M001 cursor or resyncs bounded window. Rapid project switching does not cross-route draft/messages. Observer target disconnect leaves chat usable.

## 9. Compatibility and migration

When server lacks chat capability, panel is hidden/unavailable; local session operation remains unchanged. Optional local draft persistence is out of scope unless existing TUI state convention provides it trivially.

## 10. Required tests

Multi-project routing; send/reply/render; paging/resync; unread/read; composing expiry display; authorization denial; observer insert goes to chat and emits zero steer/cancel/permission responses; reconnect; focus/key regressions; bounded history.

## 11. Required verification commands

```bash
cargo test --workspace tui --no-fail-fast
cargo test --workspace collaboration --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

TUI architecture/help/keybindings and project collaboration docs.

## 13. Acceptance criteria

Project members can communicate in the correct project while observing work; reconnect/paging are deterministic; observer insert-mode cannot mutate observed session; unsupported/unauthorized states degrade safely.

## 14. Stop conditions

M001 not closed; TUI would need to own message ordering/auth; observer-mode routing requires weakening read-only invariant.

## 15. Closure evidence required

Dependency closure, reducer/routing/focus tests, observer zero-control evidence, reconnect/bounds/privacy tests, exact commands/results.

## 16. Handoff notes

Use existing command/focus/state patterns. Do not build a second chat-specific websocket or polling loop.
