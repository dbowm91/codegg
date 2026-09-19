# Team Collaboration Corrective M004 — Shared-Session Controller Lease

Status: implemented (closure at
plans/closure/team-collaboration-corrective/004-status.md;
implementation `1bc967c1`)

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M004`

Long-term requirements: `plans/000-long-term-specification.md#15`, `#22`, `#26`, `#27`.

Applicable ADR: `plans/adrs/ADR-0007-shared-session-control-ownership.md`. Primary class: capability.

Hard dependency: M001 strict closure.

## 1. Objective

Make active-turn human control explicit. The principal that successfully submits a turn controls steer/cancel and permission/question responses for that turn until terminal state or an explicit authorized transfer/takeover.

## 2. Why this milestone is blocked

Controller semantics must cover every network path that can steer/cancel/respond. M001 must first prove there is no REST compatibility bypass.

## 3. Current implementation evidence

`TurnSubmit`, `TurnSteer`, and `TurnCancel` are all `ViaSession + agent.invoke`. The session runtime has one `active_turn` lock, so concurrent turns are rejected, but the lock records no human controller. Observer TUI hard-blocks control, yet another Contributor outside observer mode can still steer/cancel. Permission/question Core descriptors are global and legacy HTTP registries expose session-scoped pending items.

## 4. Invariants that must not regress

- Controller state only narrows existing project/session capability.
- At most one effective controller principal exists per active turn.
- Turn completion releases control.
- Observer/chat access never grants control.
- Disconnect never silently transfers control.
- Transfer/takeover is explicit, revisioned, and audited.
- Revoked/suspended principal cannot exercise a stale controller lease.

## 5. Scope

In: durable/recoverable controller record, automatic acquisition on successful turn submit, steer/cancel/permission/question gating, request/transfer/release/takeover protocol, projection/presence metadata, TUI indicator/commands, restart/reconnect/race tests.

Out: concurrent turns, collaborative editor semantics, permanent session ownership, implicit control transfer from chat, changing worktree isolation.

## 6. Required production changes

Implement the ADR-0007 controller record keyed by canonical session+turn with principal, origin client metadata, revision and lifecycle timestamps/provenance. Acquisition must be atomic with accepting the turn or otherwise rollback-safe: a failed turn submission cannot leave a controller lease.

Add controller authorization after the existing capability decision for `TurnSteer`, `TurnCancel`, and pending permission/question response. Pending IDs must resolve to the owning session/turn before this check.

Add Core operations for `SessionControlGet`, request/suggest, transfer, release where valid, and forced takeover. A control request is inert notification/state only. Transfer requires current controller plus recipient eligibility. Forced takeover requires Maintainer/Owner-equivalent policy and a bounded reason, and emits audit.

Expose controller principal/coarse status in safe session projection/presence; do not expose credentials or device secrets.

## 7. Ordered work packages

A. Add schema/store/reconciliation state machine and migration.
B. Integrate atomic acquisition/release with turn lifecycle.
C. Gate steer/cancel and permission/question responses.
D. Add request/transfer/takeover Core protocol + authorization/audit.
E. Add projection/presence and TUI controller indicator/actions without weakening observer block.
F. Add reconnect/restart/contention/adversarial tests.

## 8. Failure, cancellation, restart, contention semantics

Simultaneous transfer/takeover uses revision/CAS; one wins and stale writers fail. Turn terminal transition wins over later transfer and releases the lease. Controller disconnect preserves lease through reconnect grace; no other Contributor gains control automatically. Restart reconciles lease against active-turn state and trustworthy origin attribution. Ambiguous active-turn controller fails closed until explicit authorized takeover.

## 9. Compatibility and migration

Inactive sessions need no record. Solo/LocalOwner behaves as before because the submitter automatically controls the turn. Existing active turns during upgrade derive controller only from trustworthy origin attribution; otherwise recovery takeover is required.

## 10. Required tests

Two Contributors: second cannot steer/cancel/respond; same principal second device can resume control; controller transfer succeeds; unauthorized transfer fails; Maintainer/Owner forced takeover succeeds and is audited; Contributor force fails; controller revocation denies; terminal turn releases; submit/control race; transfer/completion race; restart reconciliation; observer and Viewer+chat remain unable to control; REST compatibility path cannot bypass.

## 11. Required verification commands

- focused daemon turn/control tests
- `cargo test --test presence_m003_observation`
- `cargo test --test identity_m003_daemon_authorization`
- permission/question integration tests
- `python3 scripts/check_authorization_matrix.py`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update `architecture/authorization.md`, session/turn architecture, `architecture/presence.md`, `architecture/tui.md`, server permission/question docs, and help text for control request/transfer/takeover.

## 13. Acceptance criteria

When Alice starts a turn, Bob may observe and chat but cannot steer, cancel, or answer Alice's pending control items. Alice can transfer to Bob; an eligible Maintainer/Owner can explicitly recover control. Every transition is attributable and survives reconnect without implicit handoff.

## 14. Stop conditions

Do not implement controller checks only in the TUI. Do not use client ID as the primary human authority. Do not infer takeover from timeout alone. Stop if any legacy route can still mutate the active turn outside the controller gate.

## 15. Closure evidence required

State-machine/recovery matrix, exact authorization and race tests, audit examples without sensitive content, upgrade behavior for pre-existing active turns, and proof observer/chat grants do not widen control.

## 16. Handoff notes

Keep the lease turn-scoped. Parallel mutation work remains separate sessions/worktrees; this milestone only makes exceptional shared-session control safe.
