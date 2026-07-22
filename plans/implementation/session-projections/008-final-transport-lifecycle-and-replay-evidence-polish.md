# Session Projections Milestone 008 — Final Transport Lifecycle and Replay Evidence Polish

Status: ready for handoff

Repository baseline: `8b547a3d02e571a480a826f5dea9c81d79d95cc4` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Corrected closure record:

- `plans/closure/session-projections/007-status.md`

Primary class: correctness polish / task lifecycle / adapter verification / closure reconciliation

## 1. Objective

Finish the frontend-neutral session-projections transport line with one tightly scoped polish pass. Preserve the valid M006 and M007 production work while closing the residual lifecycle and evidence gaps found during post-M007 inspection.

M007 correctly delivered:

- owned and joined Unix client/raw-forwarder lifecycle;
- generation-tagged `/tui` raw routing with final-writer stale rejection;
- connection-local projection lifecycle fault seams;
- real blocked-response ordering tests for Unix, `/core`, and `/tui`;
- real foreign-operation rejection coverage;
- real reconnect/resume fixtures;
- corrected protocol-version expectations and transport test counts.

The remaining work is narrow:

1. `/core` and `/tui` retain their send, receive, and raw-event task handles but abort sibling tasks without awaiting the aborted handles before connection cleanup completes;
2. adapter-level critical-delivery failure coverage does not yet exercise every approved failure class against an installed staged daemon subscription;
3. reconnect tests prove replay range metadata and a subsequent live subscription ID, but do not directly prove exact replay envelope identity, monotonic sequence continuity, absence of duplicates, and first-live sequence `replay_end_seq + 1`;
4. M007 closure claims deterministic joined cleanup and a complete adapter failure matrix more broadly than the code and tests currently support.

The milestone succeeds when WebSocket connection tasks terminate deterministically, every material critical-delivery failure class is demonstrated through production adapter setup, reconnect tests inspect exact envelopes and sequence continuity, and the final closure record matches executable evidence exactly.

## 2. Scope boundaries

### In scope

- `/core` and `/tui` send/receive/raw-event task ownership and joined teardown.
- Connection cancellation before task abort/join.
- Idempotent teardown when any one connection task exits first.
- Production-adapter fault tests for queue saturation/timeout, writer failure, cancellation, serialization where practical, and disconnect during staged setup.
- Assertions that failed setup removes connection ownership, daemon subscription state, receiver availability, and forwarder tasks.
- Exact replay envelope identity and sequence assertions for Unix, `/core`, and `/tui` reconnect tests.
- Duplicate/gap/reordering negative assertions at replay-to-live handoff.
- Static guard extension for abort-without-await WebSocket task patterns.
- Correction of M007 closure, roadmap, registry, and final M008 closure evidence.

### Explicitly out of scope

- Projection DTO, snapshot, or event schema changes.
- Projection protocol version changes.
- SQLite schema, retention, checkpoint, cursor, or sequence-authority changes.
- Replay service or event-log replacement.
- New frontend features or TUI rendering work.
- Team authorization, presence, chat, or organization policy.
- Cross-daemon replay replication.
- Removal of version-4/raw compatibility.
- General server authentication redesign.
- General workspace-wide lint cleanup outside files touched by M008.

## 3. Required invariants

- Every task created for a `/core` or `/tui` connection is retained by its connection scope.
- When any connection task exits, the connection cancellation token is cancelled before sibling cleanup begins.
- Every remaining connection task is awaited after normal completion or abort.
- No send, receive, raw-event, or projection-forwarder task remains scheduled after the connection handler returns.
- Cleanup is safe when the writer exits first, reader exits first, raw-event source closes first, cancellation fires, or subscription rollback is already in progress.
- No connection-state lock is held while awaiting socket I/O or joining a task.
- A staged subscription cannot become `Live` before successful canonical response delivery.
- Queue timeout, writer failure, cancellation, serialization failure, and disconnect before activation cannot leave connection ownership or daemon subscription state behind.
- A failed staged receiver cannot be taken a second time.
- Reconnect replay contains exactly the committed events after the supplied cursor, once each and in ascending sequence order.
- The first live envelope after replay has sequence `replay_end_seq + 1` for the same stream.
- No replay envelope is delivered again through the live path.
- Stream identity and new subscription identity remain distinct and are asserted independently.
- Closure claims, test names, counts, commands, commits, and residual failures match the repository.

## 4. Work package A — Joined `/core` and `/tui` connection teardown

Primary file:

- `src/server/ws.rs`

Required changes:

1. Replace the current sibling `abort()`-only branches in `upgrade_core_ws` and `upgrade_tui` with one structured teardown helper or equivalent explicit sequence.
2. On the first terminal task result:
   - cancel the connection token;
   - close/drop connection-local outbound senders where useful;
   - abort any still-running sibling tasks;
   - await every retained task handle;
   - classify `JoinError::is_cancelled()` as expected cleanup;
   - log only abnormal panic/error termination with connection identity;
   - continue projection-state and daemon-unsubscribe cleanup only after the connection tasks are terminated.
3. Ensure the task whose completion selected the branch is not polled/awaited twice incorrectly. Use `JoinSet`, `Option<JoinHandle<_>>`, or a helper that tracks completed handles explicitly.
4. Apply the same ownership model to `/core` and `/tui`; avoid two divergent cleanup implementations.
5. Preserve the existing bounded channel capacities and writer-receipt semantics.
6. Ensure connection cancellation wakes:
   - pending staged critical sends;
   - writer lifecycle gates;
   - raw event receivers;
   - projection receiver-forwarders.
7. Do not hold `TuiSessionState`, `ProjectionConnectionState`, filter, or raw-delivery-gate locks while joining tasks.

Recommended shape:

```text
ConnectionTasks {
    send: Option<JoinHandle<()>>,
    recv: Option<JoinHandle<()>>,
    raw: Option<JoinHandle<()>>,
}

first task exits
    -> connection_cancel.cancel()
    -> abort remaining live handles
    -> await all handles
    -> drain/join projection subscriptions
    -> daemon unsubscribe
    -> return
```

Equivalent `JoinSet` or structured-concurrency implementations are acceptable.

Required tests:

- `/core` peer close terminates and awaits send, receive, and raw tasks;
- `/tui` peer close terminates and awaits send, receive, and raw tasks;
- writer failure follows the same joined teardown path;
- raw event source closure follows the same joined teardown path;
- cancellation while a staged response is paused does not leave a writer/receiver task;
- repeated connect/disconnect cycles return connection-task counters or drop probes to baseline;
- client A teardown does not stop or perturb client B;
- cleanup remains idempotent when projection rollback removed subscriptions before connection teardown.

## 5. Work package B — Production-adapter critical failure matrix

Primary files:

- `src/core/transport/projection.rs`
- `src/core/transport/daemon_socket.rs`
- `src/server/ws.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`
- `tests/projection_transport_real.rs`

Use the existing connection-local `ProjectionLifecycleSeam`. Extend it only where a required failure cannot be represented deterministically. Do not add process-global mutable test state.

### Required `/core` scenarios

- queue remains saturated until the bounded critical-send timeout;
- writer receipt reports `WriterClosed` after receiver installation;
- connection cancellation wins while response delivery is pending;
- disconnect occurs after receiver installation and before activation;
- serialization failure is exercised through the nearest production-shaped staged-response seam possible;
- failure immediately before activation rolls back the already delivered staged subscription;
- duplicate cleanup/rollback converges to one terminal state.

### Required `/tui` scenarios

- projection/control queue remains saturated until timeout;
- writer receipt reports `WriterClosed` after receiver installation;
- connection cancellation wins while response delivery is pending;
- disconnect occurs while the canonical snapshot/replay is paused;
- serialization failure is exercised through a production-shaped staged delivery helper or an explicit adapter test seam;
- failure immediately before activation removes subscription ownership and daemon state;
- duplicate cleanup/rollback is harmless.

### Required Unix scenarios

- writer/write/flush failure after receiver installation;
- peer disconnect before canonical response completion;
- cancellation and response-completion race;
- failure immediately before activation;
- repeated cleanup is harmless.

### Assertions required for every staged-subscription failure test

- the client receives no successful canonical subscription response;
- no live projection envelope is emitted;
- connection-local state no longer owns the subscription;
- daemon active subscription count returns to its baseline;
- `take_subscription_receiver` cannot recover the failed receiver again;
- the connection-owned projection forwarder terminates;
- task/drop probes return to baseline;
- a second cleanup call is a no-op;
- an unrelated client remains live where the scenario uses two clients.

### Serialization testing guidance

The canonical production projection responses are normally serializable. Do not distort protocol DTOs solely to create an impossible serialization failure. Acceptable approaches include:

- a test-only/injected serializer failure at the adapter's staged serialization boundary;
- a generic staged-send helper exercised with a failing serializable type while an actual daemon subscription is installed by the fixture;
- a connection-local lifecycle fault classified as `Serialization` before enqueue, provided the test proves full staged-subscription rollback.

Document which boundary is represented and why it is production-equivalent.

## 6. Work package C — Exact reconnect and replay-to-live proof

Primary files:

- `tests/projection_transport_real.rs`
- `src/core/transport/daemon_socket_integration_tests.rs`

Strengthen the Unix, `/core`, and `/tui` reconnect fixtures so they inspect complete replay and live envelopes rather than only event counts, range metadata, or subscription IDs.

For each transport:

1. subscribe and record:
   - stream ID;
   - initial subscription ID;
   - acknowledged/persisted cursor;
2. disconnect and wait until the prior daemon subscription is removed;
3. publish a bounded missing range with unique event identities, for example distinct turn IDs;
4. reconnect with a fresh daemon-issued connection identity;
5. resume from the prior cursor;
6. assert:
   - the resumed descriptor has the same stream ID;
   - the new subscription ID differs from the prior subscription ID;
   - replay range metadata matches the expected first and last sequence;
   - replay event sequence numbers are exactly the expected ascending sequence vector;
   - replay event identities are exactly the expected unique turn/event IDs;
   - each expected event appears once;
7. publish a uniquely identifiable live event at or immediately after the replay handoff;
8. inspect the full live envelope and assert:
   - the stream ID matches the replay stream;
   - the subscription ID matches the new subscription;
   - its event sequence is `replay_end_seq + 1`;
   - its identity is the expected live event;
9. poll for a bounded quiet period and assert no duplicate replay or live envelope follows;
10. repeat at least one fixture with publication while replay response delivery is paused to prove the handoff remains gap-free under a real race;
11. disconnect during replay in one adapter fixture and prove daemon/task cleanup returns to baseline.

Do not treat replay batch length plus a later subscription ID as sufficient exactness evidence.

## 7. Work package D — Static lifecycle and evidence guards

Primary file:

- `scripts/check_projection_transport_lifecycle.py`

Required extensions:

- reject `/core` or `/tui` teardown branches that call `.abort()` on retained connection-task handles without a corresponding awaited join path;
- require connection cancellation before joined task cleanup;
- continue enforcing owned Unix client/raw-forwarder tasks;
- continue enforcing final-writer TUI raw-generation checks;
- continue rejecting unbounded server WebSocket queues;
- continue rejecting direct live-transition bypasses;
- check that the final M008 closure record references the real transport suite and exact executable count where mechanically practical.

A textual guard is acceptable, but it must target stable helper names or structured task-owner types rather than fragile line numbers.

Static guards supplement runtime tests; they do not replace drop probes or real transport fixtures.

## 8. Work package E — Documentation and final closure reconciliation

Required documents:

- `plans/closure/session-projections/007-status.md`
- `plans/subsystems/session-projections-roadmap.md`
- `plans/registry.md`
- new `plans/closure/session-projections/008-status.md`

Required changes:

1. Keep M007's valid production outcomes accepted.
2. Record M007 as conditionally closed until M008 completes because:
   - WebSocket sibling connection tasks are aborted but not awaited;
   - adapter-level failure evidence is incomplete relative to the approved M007 plan;
   - reconnect assertions do not yet prove envelope-level sequence continuity and absence of duplication.
3. Do not rewrite M007 as a failure; identify M008 as a final polish closure.
4. After implementation, create M008 closure only when:
   - joined teardown is present in `/core` and `/tui`;
   - the adapter failure matrix is complete or every intentionally impossible case is narrowly and truthfully justified;
   - exact reconnect assertions pass on all three transports;
   - focused suites and guards pass;
   - no unresolved high or medium M008 finding remains.
5. Reconcile exact test names and executable counts after implementation.
6. Record the implementation commit and closure commit exactly.
7. Separate unrelated workspace-wide lint failures from milestone-local results.
8. Return the roadmap and registry to strict closed status only through the M008 closure record.

## 9. Required verification matrix

At minimum run:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
cargo test -p codegg --lib core::transport::projection --all-features -- --nocapture
cargo test -p codegg --lib server::ws --all-features -- --nocapture
cargo test -p codegg --lib core::transport::daemon_socket --all-features -- --nocapture
cargo test --test projection_transport_real --features server -- --test-threads=1 --nocapture
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

Also run any new focused task-ownership/drop-probe test module introduced by M008.

Repository-wide clippy may still report pre-existing EggLSP `question_mark` findings. Record those separately; M008 must not introduce new warnings in its changed files or focused gates.

## 10. Acceptance criteria

- `/core` connection teardown cancels and awaits every retained connection task.
- `/tui` connection teardown cancels and awaits every retained connection task.
- No abort-only sibling task cleanup remains in the two projection-capable WebSocket adapters.
- Repeated WebSocket connect/disconnect and staged-failure cycles return task/drop counters to baseline.
- Client A teardown or failure does not perturb client B.
- Queue timeout, writer failure, cancellation, disconnect during install, pre-activation failure, and serialization-equivalent failure have production-adapter staged-subscription tests where applicable.
- Every staged failure removes connection ownership and daemon subscription state, terminates the forwarder, prevents receiver reuse, and is idempotent.
- Unix, `/core`, and `/tui` reconnect tests assert exact replay event sequences and identities.
- The first live event after replay has the expected next sequence and unique identity.
- No replay or live event is duplicated during the handoff.
- Stream identity remains stable while subscription and connection identities change.
- The lifecycle static guard rejects abort-without-await WebSocket cleanup.
- Focused transport, replay, disclosure, TUI, and lifecycle suites pass.
- Closure test names and counts match executable results.
- No unresolved high or medium M008 finding remains.
- `plans/closure/session-projections/008-status.md` records exact implementation and closure commits.
- The registry contains no dependency-ready projection plan only after M008 is strictly closed.

## 11. Handoff order

Recommended implementation sequence:

1. Add shared joined WebSocket connection-task teardown.
2. Add task/drop probes and peer-close/writer-close tests.
3. Complete `/core` staged failure scenarios.
4. Complete `/tui` staged failure scenarios.
5. Complete Unix residual staged failure scenarios.
6. Strengthen reconnect fixtures to inspect full envelopes and sequences.
7. Extend the lifecycle static guard.
8. Run the required verification matrix.
9. Correct M007 evidence and write the M008 closure record.
10. Return the roadmap and registry to strict closed status only after acceptance.

## 12. Final completion definition

This line of work is complete when all three production transports are bounded and connection-owned at both subscription and task levels, every retained connection task is deterministically terminated, critical setup failures are proven against staged daemon subscriptions, replay-to-live continuity is demonstrated at envelope and sequence level without gaps or duplicates, and the planning/closure record matches the executable repository exactly.
