# TUI Live Work Visibility Corrective M006 — Conditional Closure

Status: **conditionally closed — multi-worker timeout/restart and manual receipt outstanding**
Date: 2026-10-20 (follow-up evidence)
Class: **capability**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/006-real-daemon-to-tui-acceptance-and-recovery.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Qualified dependencies: M001–M005 closed; ADR-0041 and ADR-0043 accepted
Implementation commit: rebased branch; see follow-up commit and exact-head evidence below.
Evidence amendments: `7bbdb81` — production-bridge child events, late-attach snapshot, and same-project session-isolation regression; follow-up worker-latch, parent-completion, cancellation, and observer-denial evidence is being qualified on this branch.

## Disposition

The deterministic Unix-socket regression now exercises the real daemon,
SQLite-backed job store, `SocketCoreClient`, default TUI subscription, daemon
event publication, projection stream application, actual `App::on_key`
routing, painted `/sessions` and `Ctrl+T` surfaces, job detail, cancellation,
and durable cancellation reconciliation. It proves two child identities and
two jobs are visible under the subscribed session and that job inspection and
cancel remain identity-keyed.

The follow-up adds a deterministic held provider behind the real
`SubagentJobExecutor` and `SubAgentPool`. A late ordinary TUI attaches over the
Unix socket after the real worker reaches the provider latch. Its authorized
snapshot, task rows, and painted `/sessions` dialog contain the same child ID
and originating turn. Parent completion is published while the worker remains
held; the child stays Running. Owner cancellation then travels back through the
socket and both the durable job and projected child reach terminal state. This
qualifies one held worker through the socket/UI path. It does not qualify two
concurrent real workers, an actual parent timeout/stop controller path, or
restart with in-flight durable work. This environment did not provide an
interactive development terminal, so no manual receipt is claimed. M006
remains conditionally closed under its explicit stop rule.

## Scenario disposition

| Scenario | Result | Evidence / residual |
|---|---|---|
| A. Same attached session shows two children and separate jobs in both surfaces; inspect and supported cancel | **Partial pass** | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` exercises real socket job submissions plus deterministic child events through the daemon bridge, paints exact IDs, and verifies job inspect/cancel. `late_socket_tui_attaches_to_held_real_scheduled_subagent` adds one real worker: the late TUI's snapshot, task rows, and painted `/sessions` dialog contain its running child. Two concurrent real children with independent jobs are not yet covered. |
| B. Late attach, simultaneous clients, and cross-session child isolation | **Partial pass** | Deterministic socket/TUI coverage attaches a second ordinary TUI after two child events and jobs, verifies all four IDs in its initial snapshot, and verifies a second session in the same project is empty. The new held-worker test attaches an ordinary TUI over the socket after a real worker is at its provider latch and finds the live child in the initial snapshot and `/sessions` dialog. Concurrent convergence of two clients during worker transitions and unauthorized subscription are still unqualified. |
| C. Parent completion/timeout, queued cancellation, new-turn reuse, eventual worker terminal | **Partial pass** | `late_socket_tui_attaches_to_held_real_scheduled_subagent` holds a real scheduled worker, publishes parent completion while it remains Running, sends owner `JobCancel` through the socket, and waits for durable job and projected child terminal states. Actual parent timeout/stop controller behavior, queued-before-dequeue cancellation, two-worker generation reuse, and restart recovery remain unqualified. |
| D. Disconnect gap, daemon restart, ephemeral child reconciliation, duplicate terminal | **Partial pass** | After the held real worker reaches terminal state, the test constructs a fresh daemon on the same SQLite database and runs `initialize_recovery_sequence`; it verifies the canceled durable job remains terminal and no in-memory child is resurrected as Running. Existing fresh-snapshot/reconnect and cold-runtime tests cover component behavior. Restarting while durable work is still in flight after the worker is gone, cursor-gap recovery, and duplicate/reordered terminal events remain unqualified. |
| E. Modal refresh, exact job activation, cancel, observer control | **Partial pass** | Existing row removal/generation races and socket job detail/cancel tests remain. The held-worker test verifies owner cancellation reaches a real worker over socket; `observer_cannot_steer_cancel_or_mutate_through_daemon` includes opaque `JobCancel` denial before lookup. Foreign-session details and the full socket observer denial matrix remain unqualified. |
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
record why an equivalent target satisfies it) and exercise two real concurrent
children through the socket-attached painted TUI, late attachment while those
workers are held, cross-session and unauthorized-subscribe denial, parent
timeout/stop after child start, queued cancellation, reconnect cursor-gap and
daemon restart reconciliation, and the full observer cancellation/detail
denial matrix. Current evidence separates real worker/snapshot behavior from
socket/UI behavior; it does not prove their concurrent convergence. The manual
development-terminal receipt also remains required.

## Unblock audit

M001–M005 are closed, so M006 was eligible and is now conditionally closed per
its stop/closure rule. No later plan in this corrective line is registered;
the remaining full-acceptance evidence is a re-entry condition, not an
unblocked successor milestone.
