# Session Projections Milestone 008 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/008-final-transport-lifecycle-and-replay-evidence-polish.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-8--final-transport-lifecycle-and-replay-evidence-polish`

Repository baseline reviewed: `8b547a3d02e571a480a826f5dea9c81d79d95cc4`

Implementation commit:

- `6975050af530eb5bd7a640c1f7ac9a31859dfda3` — shared joined WebSocket
  teardown, staged production-adapter failure fixtures, exact Unix/
  `/core`/`/tui` replay-to-live assertions, and lifecycle guard extension.

Closure evidence commit:

- pending hash reconciliation for this closure record

## 1. Executive finding

M008 is complete. `/core` and `/tui` now retain one structured owner for their
send, receive, and raw-event tasks. The first task to terminate cancels the
connection, aborts remaining siblings, awaits every retained handle, and only
then allows projection and daemon subscription cleanup to run. Expected
cancellation joins are quiet; abnormal task termination is logged with the
connection and task identity.

Unix, `/core`, and `/tui` staged-subscription fixtures now exercise the
material queue, writer, cancellation, serialization-equivalent, disconnect,
and pre-activation failure boundaries against real daemon subscriptions.
Reconnect fixtures inspect complete replay/live envelopes and prove stable
stream identity, changed subscription identity, exact replay sequence and
turn identity, first-live sequence `replay_end_seq + 1`, and bounded absence of
duplicates. The `/core` fixture also publishes while replay response delivery
is paused.

No M008 high or medium finding remains. The session-projections roadmap and
planning registry therefore return to strict closed status.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `/core` connection tasks are owned and joined | `ConnectionTaskSet` in `src/server/ws.rs`; `connection_task_set_cancels_aborts_and_joins_all_tasks`; lifecycle guard | pass | Cancellation precedes sibling abort and every retained handle is awaited. |
| `/tui` uses the same teardown model | `upgrade_tui` uses `ConnectionTaskSet`; real TUI lifecycle/failure fixtures | pass | No divergent abort-only cleanup remains in either projection adapter. |
| Cleanup is safe when any sibling exits first | shared first-exit selection and idempotent projection cleanup; `server::ws` focused suite | pass | The selected handle is consumed once; remaining handles are abort-and-await joined. |
| Connection cancellation wakes pending setup work | connection token passed through staged sends, lifecycle checkpoints, writer gates, and forwarders | pass | Connection cleanup runs after task termination and before daemon unsubscribe. |
| Production-adapter critical failure matrix | `real_core_staged_failure_matrix_rolls_back_every_material_class`, `real_tui_staged_failure_matrix_rolls_back_every_material_class`, `socket_staged_failure_matrix_rolls_back_every_material_class` | pass | Seven connection-local scenarios per adapter; daemon active count returns to baseline and no live event escapes. |
| Post-enqueue and pre-activation rollback is truthful | matrix tests plus `real_*_failed_critical_delivery_rolls_back_daemon_subscription` | pass | A response may already be on the wire after the post-enqueue/pre-activation boundary; those cases assert rollback, no live traffic, and cleanup instead of claiming response absence. |
| Receiver ownership is single-take and cleanup is idempotent | transport install/rollback paths; `projection_replay_transport_isolation`; Unix cleanup and failed-install tests | pass | Failed receiver installation cannot be recovered as a second transport-owned receiver. |
| Unix reconnect is exact | `socket_reconnect_replays_exact_missing_range_then_live` | pass | Exact `[1, 2]` sequence and turn identities; new subscription, same stream; live sequence 3; no duplicate. |
| `/core` reconnect is exact and race-safe | `real_core_reconnect_replays_exact_missing_range_then_live` | pass | Replay response is paused while a live event is published; handoff remains gap-free. |
| `/tui` reconnect is exact | `real_tui_reconnect_replays_exact_missing_range_then_live` | pass | Exact replay payload identities and sequence continuity are asserted on typed envelopes. |
| Raw compatibility and protocol meaning remain stable | no protocol/schema files changed; existing foreign-operation and raw-isolation tests | pass | Version-4/raw compatibility and projection protocol version are unchanged. |
| Abort-without-await regression is guarded | `scripts/check_projection_transport_lifecycle.py` | pass | The guard checks the shared owner, cancellation-before-join path, awaited handles, and adapter bodies. |
| Planning closure is reconciled | M007 conditional note, this record, roadmap, and registry | pass | M008 is the strict closure authority; no projection plan remains ready or blocked. |

## 3. Production implementation evidence

- `ConnectionTaskSet` in `src/server/ws.rs` owns the `/core` and `/tui`
  send, receive, and raw-event `JoinHandle`s. It selects the first terminal
  task once, cancels the connection, aborts remaining siblings, and awaits
  each handle before projection state is drained or daemon subscriptions are
  removed.
- The existing bounded control, projection, and raw queues remain unchanged.
  Writer receipts and the final TUI raw-generation check remain in the writer
  boundary.
- The real adapter fixtures use the existing connection-local
  `ProjectionLifecycleSeam`. Serialization-equivalent failure is injected at
  the staged pre-enqueue boundary so production DTOs remain naturally
  serializable and the wire contract is not distorted.
- Replay helpers now publish explicit source sequence values for the missing
  range and live successor. Assertions inspect `ProjectionEnvelope` payload
  identities rather than relying on outer optional metadata that is not
  retained by the canonical projection publication transform.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk cargo check -p codegg --all-features
rtk python3 scripts/check_projection_transport_lifecycle.py
rtk cargo test -p codegg --lib server::ws --all-features -- --nocapture
rtk cargo test -p codegg --lib socket_staged_failure_matrix_rolls_back_every_material_class --all-features -- --nocapture
rtk cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture
rtk cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture real_core_staged_failure_matrix_rolls_back_every_material_class
rtk cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture real_tui_staged_failure_matrix_rolls_back_every_material_class
rtk cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
rtk git diff --check
```

### Results

- `projection_transport_real`: 20 listed and 20 passed; 15 transport cases
  and 5 shared secret-scan tests. The full suite includes both new staged
  failure-matrix tests and the exact core/TUI reconnect tests.
- `server::ws`: 8 focused tests passed, including the task-owner drop probe.
- `daemon_socket` focused suite: 20 tests passed, including exact Unix replay
  and the seven-scenario staged failure matrix.
- The focused lifecycle guard and formatting/diff checks passed.
- The initial workspace check passed with four pre-existing warnings in TUI
  persistence/app code; M008 did not introduce warnings in its changed
  production files.

The broader repository verification commands retained by M007 remain
historical evidence. M008's changed-surface gates above are the executable
closure gates for this polish milestone; unrelated workspace-wide clippy
findings remain outside M008 scope.

## 5. Invariant review

- Every `/core` and `/tui` connection task is retained by one owner and joined
  before connection cleanup returns.
- Cancellation occurs before sibling teardown, and no projection state lock is
  held while a task handle is awaited.
- Critical staged delivery still precedes activation; rollback and daemon
  unsubscribe converge safely when cleanup is repeated.
- Reconnect preserves stream identity while issuing fresh subscription and
  connection identities, and the replay-to-live boundary is exact and
  duplicate-free in all three production transports.
- Outbound queues remain bounded and raw TUI route generations remain checked
  at the final writer boundary.

## 6. Failure and recovery review

- Queue, writer, cancellation, serialization-equivalent, disconnect, and
  pre-activation failures are injected through connection-local seams against
  installed daemon subscriptions.
- Post-enqueue and pre-activation tests explicitly distinguish an already
  delivered canonical response from a failed live activation and require
  daemon ownership removal and no live projection traffic.
- A forwarder is cancelled and joined by projection cleanup; the existing
  single-take receiver tests prevent a failed receiver from being recovered a
  second time.
- Exact replay tests include a paused response/live-publication race and a
  bounded quiet period after the first live envelope.

## 7. Migration and compatibility review

No storage schema, replay authority, cursor, sequence authority, projection
DTO, or protocol version changed. Version-4/raw compatibility remains
available. The changes are limited to task lifecycle ownership, test seams,
replay evidence, and static/documentation guards.

## 8. Security and contention review

Subscription ownership remains daemon-issued and connection-scoped; foreign
operations remain fail-closed. The failure fixtures use no process-global
mutable state. Existing bounded queue, artifact, disclosure, and raw-filter
guards remain in the focused transport suites.

## 9. Documentation and operations

- Corrected `plans/closure/session-projections/007-status.md` to preserve its
  conditional historical status while linking M008 as the strict authority.
- Updated `plans/subsystems/session-projections-roadmap.md` to strict closed
  status and linked this record.
- Updated `plans/registry.md` to remove the ready/active/blocked M008 rows and
  record the closed milestone.
- Extended `scripts/check_projection_transport_lifecycle.py` to reject
  adapter-local abort-only cleanup and require the shared joined owner.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | No high or medium M008 finding remains. |
| repository baseline | Existing unrelated warnings/clippy findings outside changed files | Does not affect M008 transport correctness | Track in the existing repository baseline; do not fold into this milestone. |

## 11. Roadmap disposition

Milestone 008 is closed. The frontend-neutral session-projections roadmap and
registry are strictly closed, with no dependency-ready projection plan and no
blocked projection closure work remaining.

The exact closure evidence commit hash is recorded in the follow-up
hash-reconciliation commit for this record.
