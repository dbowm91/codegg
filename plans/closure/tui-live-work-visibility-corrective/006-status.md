# TUI Live Work Visibility Corrective M006 — Conditional Closure

Status: **conditionally closed — interactive worker/recovery receipt outstanding**
Date: 2026-10-19
Class: **capability**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/006-real-daemon-to-tui-acceptance-and-recovery.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Qualified dependencies: M001–M005 closed; ADR-0041 and ADR-0042 accepted
Implementation commit: `a735ffed21fce498a26b1f50aa7085456760ce9d`
Evidence amendment commit: `7bbdb81` — production-bridge child events, late-attach snapshot, and same-project session-isolation regression.

## Disposition

The deterministic Unix-socket regression now exercises the real daemon,
SQLite-backed job store, `SocketCoreClient`, default TUI subscription, daemon
event publication, projection stream application, actual `App::on_key`
routing, painted `/sessions` and `Ctrl+T` surfaces, job detail, cancellation,
and durable cancellation reconciliation. It proves two child identities and
two jobs are visible under the subscribed session and that job inspection and
cancel remain identity-keyed.

This environment did not provide an interactive development terminal or a
scripted delegated provider/worker that could be held while the UI was observed.
No manual terminal receipt is claimed. M006 explicitly allows conditional
closure when live TUI acceptance cannot be launched; the remaining production
worker, attach/recovery, and operator scenarios below are therefore recorded as
unqualified, not passed. M006 is conditionally closed as a bounded disposition,
not full release acceptance.

## Scenario disposition

| Scenario | Result | Evidence / residual |
|---|---|---|
| A. Same attached session shows two children and separate jobs in both surfaces; inspect and supported cancel | **Partial pass** | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job`: real socket job submissions plus deterministic `AppEvent::SubagentStarted` publications through the global bus and daemon event bridge; exact IDs are applied and painted. Job inspection/cancel goes through TUI requests and the daemon store. Child starts do not come from real worker executions. |
| B. Late attach, simultaneous clients, and cross-session child isolation | **Partial pass** | The same socket/TUI test now attaches a second ordinary TUI after the two child events and jobs are published, verifies all four identities in its initial authorized snapshot, then binds another TUI to a second session in the same project and verifies its child/job snapshot is empty. This covers late snapshot reconstruction and session isolation for deterministic child events. Both TUI clients are not held concurrently through live worker transitions, and no worker latch is involved. |
| C. Parent completion/timeout, queued cancellation, new-turn reuse, eventual worker terminal | **Not qualified end-to-end** | M003 reducer/worker tests establish stable generation semantics, but this test does not execute two real child workers across those races. |
| D. Disconnect gap, daemon restart, ephemeral child reconciliation, duplicate terminal | **Partial component evidence** | Fresh snapshot/reconnect and cold runtime registry behavior have focused tests; no combined real-worker restart trace or cursor-gap TUI test was run. |
| E. Modal refresh, exact job activation, cancel, observer control | **Partial pass** | Row removal/generation races and observer-disabled cancel are tested. Socket test verifies owner cancel reaches `JobStore`; the full foreign-project/observer daemon denial matrix remains policy-test evidence. |
| F. Unavailable versus empty, bounded overflow | **Pass at component level** | M001/session dialog tests distinguish unavailable/loading/empty; projection caps and omission counts are covered. No forced socket outage painted in the same end-to-end run. |
| Manual operator receipt | **Not available** | No interactive terminal and no real provider-backed delegated work were available in this coding session. Receipt was not fabricated. |

## Verification executed

| Command | Result |
|---|---|
| `rtk cargo test -p codegg --lib default_tui_socket_path_installs_authorized_session_snapshot_and_live_job --locked` | pass (1) |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy -p codegg --lib --tests --locked -- -D warnings` | pass |
| `rtk cargo test -p codegg-protocol --lib projection --locked` | pass (98) |
| `rtk cargo test -p codegg --lib subagent --locked` | pass (120) |
| focused `/sessions`, task-overlay, side-panel, detail, and session-work suites | pass; exact results recorded in M003–M005 closure records |
| `rtk python3 scripts/check_projection_transport_isolation.py` | pass |
| `rtk python3 scripts/check_projection_transport_lifecycle.py` | pass |
| `rtk python3 scripts/check_tui_transport_authority.py` | pass |
| `rtk git diff --check` | pass |
| `rtk bash scripts/verify.sh quick` | pass (exact implementation tree including the late-attach/session-isolation regression) |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `rtk git diff --check` | pass |

## Required re-entry evidence

Before claiming full M006 acceptance or release readiness, add the named
real-daemon-to-TUI integration target required by the implementation plan (or
record why an equivalent target satisfies it) and exercise a deterministic
worker/provider latch, two real concurrent children, late attachment while
those workers are held, cross-session and unauthorized-subscribe denial,
parent-turn transition, reconnect/restart reconciliation, and observer
cancellation denial. The current library test proves late initial-snapshot
reconstruction and isolation for deterministic child events; do not infer real
worker lifecycle or concurrent client convergence from that result. The
manual development-terminal receipt also remains required.

## Unblock audit

M001–M005 are closed, so M006 was eligible and is now conditionally closed per
its stop/closure rule. No later plan in this corrective line is registered;
the remaining full-acceptance evidence is a re-entry condition, not an
unblocked successor milestone.
