# Presence and Observation Milestone 002 — TUI Collaborator and Presence Surface

Status: implemented

Closure: `plans/closure/presence-observation/002-status.md`

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/presence-observation-roadmap.md#M002--TUI-collaborator-and-presence-surface`

Long-term requirements: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `#14-presence-and-real-time-team-awareness`, `#25-tui-target-behavior`; roadmap Phase 7.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Render daemon-owned authorized presence in each project tab through a bounded collaborator panel/header and navigation affordance, preserving the TUI as a projection rather than a presence owner.

## 2. Why this milestone is ready

Unblocked by M001 closure (`plans/closure/presence-observation/001-status.md`). Multi-project TUI/session state and frontend-neutral projection controller are already closed.

## 3. Current implementation evidence

Project tabs/session lists/activity projections exist; the canonical target includes collaborators/presence but no production team panel is registered. The TUI already consumes daemon/project-specific state and must not infer collaborators from local process state.

## 4. Invariants that must not regress

No TUI-side authority or durable presence; events route to the correct project tab; inactive tabs remain bounded; unauthorized/absent data renders identically; focus/key behavior does not mutate sessions unexpectedly.

## 5. Scope

In: presence state adapter/reducer, project header count/status, collaborator panel/list, activity labels, navigation/focus, reconnect/resync. Out: observation action implementation (M003), project chat, membership administration, avatars/general social UI.

## 6. Required production changes

Frontend: add project-scoped presence model derived from daemon snapshots/events; render bounded entries with principal display identity, session/agent counts and high-level status permitted by policy. Protocol: consume M001 capability only. Runtime: no polling storm; existing subscription/event pipeline. Security: never render hidden project/presence data from local caches after authorization loss. Docs: keybindings/help.

## 7. Ordered work packages

A — add TUI presence reducer/state keyed by project and sequence/generation.

B — implement collaborator panel/header with stable ordering, bounds and empty/loading/error states.

C — integrate key/focus semantics (e.g. conceptual `Space p`) without hard-coding if command registry convention differs.

D — reconnect/project-switch/privacy regression tests and docs.

## 8. Failure, cancellation, restart, and contention semantics

Lag/resync replaces stale presentation from authoritative snapshot. Disconnect marks/clears according to M001 semantics. Rapid project switching does not leak one project's collaborators into another. No detached task per update.

## 9. Compatibility and migration

Older daemons lacking presence capability show an unavailable/hidden panel without breaking project tabs. No storage migration.

## 10. Required tests

Multi-project routing; ordering/bounds; stale expiry update; reconnect/resync; unauthorized/feature-absent state; identical names across projects; focus/key regression; inactive-tab resource bound.

## 11. Required verification commands

```bash
cargo test --workspace tui --no-fail-fast
cargo test --workspace presence --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

TUI architecture/keybinding/help and presence roadmap status on closure.

## 13. Acceptance criteria

Authorized user can open a project collaborator view and see bounded current identities/activity; switching/reconnect/expiry remain correct; unsupported or unauthorized data never leaks; TUI owns no presence truth.

## 14. Stop conditions

M001 not closed; UI requires new durable presence semantics; implementation bypasses canonical TUI projection/event pipeline.

## 15. Closure evidence required

Dependency closure, screenshots/text fixtures if existing test conventions permit, reducer/routing/focus tests, compatibility/privacy evidence, exact commands/results.

## 16. Handoff notes

Keep activity labels coarse and bounded; do not expose hidden reasoning or raw command content as presence.
