# Session Projections Milestone 007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/007-corrective-transport-lifecycle-and-evidence-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `dbbaabdde51db09f0c5beb704234ce1d94d01c9a`

Implementation commits:

- `9887c2d581a3d01280485523161695d08469c34f` — corrective transport lifecycle implementation, adapter seams, focused regression tests, and lifecycle static guard.

Closure record commit:

- `922333b5787944d033c004cbc184a9de06778a88` — introduced this closure record and the strict M007 planning transition.

## 1. Executive finding

Milestone 007 is strictly closed. The production Unix, `/core`, and `/tui` transport paths now own connection-scoped tasks, cancel and join them during teardown, reject stale raw traffic at the final writer boundary, and keep a subscription initializing until its canonical response is delivered. Production-adapter tests cover blocked-response ordering, rollback, foreign operations, reconnect replay, and cross-client isolation. No unresolved high or medium M007 finding remains.

M007 changes transport lifecycle and evidence only. It does not change projection storage, replay authority, reducer semantics, disclosure policy, artifact bounds, protocol DTO meaning, or the version-5 wire contract. Version-4 raw compatibility remains available where required.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Unix raw forwarding is connection-owned, cancellation-aware, and joined | `src/core/transport/daemon_socket.rs`; `core::transport::daemon_socket` tests; `check_projection_transport_lifecycle.py` | pass | Accepted-client handlers are owned by a `JoinSet`; raw forwarding has an owned handle and explicit cancel/await cleanup. |
| Repeated Unix disconnects release transport state | `projection_transport_real::unix_*` tests and daemon socket teardown tests | pass | EOF, shutdown, writer failure, and reconnect paths exercise cleanup and leave no active owned subscription. |
| `/tui` session switching invalidates stale raw events | `server::ws` route-generation unit tests; `real_tui_clients_keep_raw_sessions_isolated`; `real_tui_projection_primary_suppresses_raw_session_events` | pass | Route generation is attached to raw outbound items and checked again immediately before writer delivery. |
| Critical canonical response precedes live publication | `socket_projection_response_precedes_live_event_when_writer_is_blocked`; `real_core_projection_response_precedes_live_event_when_writer_is_blocked`; `real_tui_projection_response_precedes_live_event_when_writer_is_blocked` | pass | Each production adapter publishes while the response is deliberately blocked; no live event is observed before the response. |
| Critical delivery failure rolls back transport and daemon state | `socket_projection_failed_critical_delivery_rolls_back_daemon_subscription`; `real_core_failed_critical_delivery_rolls_back_daemon_subscription`; `real_tui_failed_critical_delivery_rolls_back_daemon_subscription`; lifecycle seam tests | pass | Receiver-install and writer failure seams are connection-local and verify active subscription cleanup. |
| Queue/full, writer-close, cancellation, serialization, timeout, and disconnect failure behavior remains bounded | `core::transport::projection` critical-send tests; `server::ws` writer tests; real adapter fault tests | pass | Existing helper coverage is retained and M007 adds production-adapter seam coverage for the lifecycle boundaries. |
| Foreign operations fail closed without disturbing the owner | `projection_transport_real::{core,tui,unix}_foreign_operations_*` | pass | Ack, resume, unsubscribe, status where supported, and artifact operations reject the foreign identity; owner A remains live. |
| Reconnect resumes exactly the missing range and transitions gap-free to live | `socket_reconnect_replays_exact_missing_range_then_live`; `real_core_reconnect_replays_exact_missing_range_then_live`; `real_tui_reconnect_replays_exact_missing_range_then_live` plus replay suites | pass | New identity receives the two missing committed events with replay range metadata, then receives the next live event. |
| Cross-client isolation is preserved | `projection_transport_real` two-client isolation and foreign-operation tests | pass | Projection-private events do not cross clients; raw compatibility remains scoped to the current route/session. |
| Protocol-version expectation is truthful | `src/tui/app/mod.rs` protocol test | pass | The assertion now expects the repository's protocol version 5; no protocol version was changed by M007. |
| Static lifecycle and transport invariants are enforced | `scripts/check_projection_transport_lifecycle.py`, existing projection isolation, WebSocket-bound, boundary, ownership, cwd, git, scheduler, and disclosure guards | pass | All listed guards passed. |
| Closure evidence matches executable tests and repository state | This record, updated plan/roadmap/registry, exact command results below | pass | The real transport integration test count is 18; replay and TUI counts are recorded from the executed binaries. |

## 3. Production implementation evidence

### Unix socket transport

Accepted client handlers are owned by the connection-serving `JoinSet` and are cancelled and joined during server shutdown. Each client owns a connection cancellation token and its raw forwarder handle. EOF, writer failure, cancellation, and shutdown all run idempotent cleanup that cancels the forwarder, awaits it, unsubscribes projection state, and releases writer/filter/receiver references.

The staged projection response path has connection-local lifecycle checkpoints around daemon subscription creation, receiver installation, control enqueue, writer receipt, and activation. A subscription cannot become live until the canonical response is successfully delivered.

### `/core` and `/tui` transports

The server state carries a deterministic, connection-local lifecycle seam. `/core` and `/tui` use it to pause or fail staged delivery in tests without production-global mutable state. Their writer paths use bounded, cancellation-aware delivery and cleanly roll back subscriptions on critical failure.

TUI raw outbound items carry a route generation. Session changes advance the generation and update the route atomically; the writer performs a final generation check under the writer gate, preventing a queued session-A item from crossing a committed switch to session B. Projection-private events remain suppressed from the raw compatibility path.

### Tests and guard

M007 added production-shaped Unix, `/core`, and `/tui` race, failure, foreign-operation, reconnect, raw-generation, and isolation fixtures in `tests/projection_transport_real.rs`, lifecycle seams in the transport/projection modules, and `scripts/check_projection_transport_lifecycle.py`. The static guard rejects detached connection tasks, unowned raw forwarders, unbounded WebSocket queues, direct activation bypasses, and missing final raw-generation checks.

## 4. Verification executed

### Commands run

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
CARGO_BUILD_JOBS=1 cargo test -p codegg-protocol --all-features -- --nocapture
CARGO_BUILD_JOBS=1 cargo test -p codegg-core --all-features -- --nocapture
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
cargo test --test projection_transport_real --features server -- --test-threads=1
cargo test --test projection_replay_daemon_protocol -- --nocapture
cargo test --test projection_replay_subscription -- --nocapture
cargo test --test projection_replay_resume -- --nocapture
cargo test --test projection_replay_restart_recovery -- --nocapture
cargo test --test projection_replay_transport_isolation -- --nocapture
cargo test --test projection_disclosure_invariants -- --nocapture
cargo test --test projection_artifact_handles -- --nocapture
cargo test --test tui -- --nocapture
cargo test --test tui_render -- --nocapture
cargo test --test tui_project_routing -- --nocapture
cargo test --test tui_project_tabs -- --nocapture
cargo test --test single_daemon_lifecycle -- --nocapture
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_projection_transport_lifecycle.py
python3 scripts/check_websocket_bounds.py
bash scripts/check-core-boundary.sh
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_git_forbidden_patterns.py
python3 scripts/check_scheduler_bypass.py
bash scripts/check_projection_disclosure.sh
git diff --check
```

### Results

- Formatting, workspace all-feature checking, the protocol clippy gate, all static guards, and the disclosure/boundary checks passed.
- `codegg-protocol`: 157 tests passed.
- `codegg-core`: 299 tests passed across its unit and integration binaries.
- Focused transport modules: projection 13, WebSocket 7, Unix daemon socket 20 tests passed.
- `projection_transport_real`: 18 tests passed with one test thread; this is the corrected executable count.
- Replay suites passed: daemon protocol 13, subscription 13, resume 9, restart recovery 8, transport isolation 7, disclosure invariants 16, and artifact handles 13.
- TUI suites passed: `tui` 164, `tui_render` 99, `tui_project_routing` 27, `tui_project_tabs` 20, and `single_daemon_lifecycle` 3.
- The repository-wide `CARGO_BUILD_JOBS=1 cargo clippy --all-features -- -D warnings` command was also attempted and failed only on pre-existing `clippy::question_mark` findings in `crates/egglsp/src/edit.rs:71`, `:103`, and `:134`. These are outside M007's changed scope and do not occur in the focused protocol or transport gates.
- The workspace check emitted existing warnings in TUI persistence/app code; they did not fail the check and were not introduced by M007.

## 5. Invariant review

- One canonical projection/replay authority remains in use; M007 adds no storage or sequence authority.
- Subscription, stream, project, session, client, and connection identities remain distinct.
- Projection-private events remain connection-owned and are not sent through generic raw broadcasts.
- Response delivery precedes activation and live publication on all three production adapters.
- Cleanup is bounded, cancellation-aware, idempotent, and awaited; no state lock is held across socket I/O or task joins.
- TUI route generations prevent stale queued raw traffic from crossing a committed session switch.
- Foreign lifecycle and artifact operations are rejected without changing the owner's state.
- Reconnect uses the persisted stream cursor and reports the exact replay range before live delivery.
- Bounded queues and artifact handles remain in force; no unbounded WebSocket channel was added.

## 6. Failure and recovery review

The tests cover peer EOF, connection shutdown, writer close/failure, cancellation, queue closure/full behavior, serialization failure, timeout, receiver-install failure, response-blocking, and disconnect during staged setup. Cleanup cancels and awaits owned tasks and removes both transport and daemon subscription state. Reconnect tests prove missing-range replay followed by a live event, while cross-client tests prove another owner's subscription remains live.

Malformed, foreign, and out-of-scope lifecycle/artifact requests fail closed. Stale raw generations are dropped at the final delivery boundary rather than relying only on pre-queue filtering.

## 7. Migration and compatibility review

No schema migration, replay-authority change, or protocol version increment was made. The repository remains on projection protocol version 5 and retains the version-4 compatibility path. The deprecated `/ws` path remains bounded. No rollback limitation was introduced beyond the existing requirement that a failed staged delivery removes the connection-local subscription.

## 8. Security review

No new security finding remains. Connection identity is not treated as subscription ownership; every foreign operation is checked against the owned subscription. Projection disclosure/redaction and bounded artifact reads remain covered by their existing suites. Route generation only reduces stale visibility. The new lifecycle seam is test-only/connection-local and does not expose payloads, secrets, or global mutable test state.

## 9. Documentation and operations

Updated:

- this closure record;
- the M007 implementation plan status and verification matrix;
- the M006 implementation-plan status to reflect its historical conditional record;
- the session-projections roadmap milestone and dependency state;
- `plans/registry.md` active, ready, blocked, and recently-closed sections.

Operationally relevant checks are now represented by `scripts/check_projection_transport_lifecycle.py` in addition to the existing projection isolation, WebSocket bound, ownership, cwd, boundary, scheduler, git-policy, and disclosure guards.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | No unresolved high, medium, or low M007 finding | None | None |
| repository baseline | Workspace-wide clippy fails on unrelated `egglsp` question-mark lints | Does not affect M007 focused gates; repository-wide lint is not fully green | Track/fix separately in the EggLSP lint backlog; do not reopen M007 |

## 11. Roadmap disposition

Milestone 007 is closed and the frontend-neutral session-projections subsystem roadmap returns to strict `closed` status. The M006 conditional record remains historical and explicitly names the findings that M007 resolved; it is not an active blocker. No future implementation plan is dependency-blocked on M007, so no additional plan status was unblocked. Deferred product work remains intentionally unregistered and outside this correctness closure.

## 12. Registry updates

`plans/registry.md` now:

- marks the session-projections roadmap `closed`;
- removes M007 from dependency-ready plans;
- removes the obsolete M006 active-closure and M007 blocked-work entries;
- records M007 under recently closed work;
- records that no dependency-ready session-projections plan remains.

The source roadmap now points M007 to this closure record and records the strict subsystem closure. The implementation plan is marked closed.
