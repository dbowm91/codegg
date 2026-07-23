# Session Projections Milestone 012 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Corrected predecessor closure:

- `plans/closure/session-projections/011-status.md`

Repository baseline reviewed: `1a93167ee3bdfdc55e4bd2746180443cc19b7c96`

Implementation commits:

- `0672044c067b4e1997c7da925e952a628b8a1b60` — TUI disconnect lifecycle correction, bounded reader/handler ownership, canonical critical-send observation, complete rollback evidence, and typed Unix socket-write evidence.
- `f046de5bee6c145d494467b36cc1a03650b220ec` — deterministic test-secret sentinel uniqueness correction discovered by the required repeated stability run.

Final reviewed code head: `f046de5bee6c145d494467b36cc1a03650b220ec`

## 1. Executive finding

M012 is closed. The `/tui` receive path now has one close-responsive socket-reader owner, a bounded sequential request-handler queue, and explicit joined ownership for the reader, handler, writer, raw-event task, and projection forwarders. Peer Close, EOF, and read errors fire connection cancellation before handler teardown can be blocked by pending projection work.

The canonical staged critical-send future now carries optional observation without changing production semantics: enqueue and receipt wait share one bounded budget, operation identity is stable, and queue metadata records maximum versus remaining capacity. Unix fixtures observe the actual production socket write result, and TUI rollback assertions use real staged subscription identities and daemon ownership. Required focused, repeated, guard, and source-audit evidence is clean. No unresolved M012 high or medium finding remains.

M011 remains conditionally closed as a historical foundation. Its documented findings are resolved by the M012 implementation and accepted here; the M011 closure record is intentionally retained unchanged as historical evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Close-responsive TUI lifecycle | D1 close-frame and D2 abrupt-drop pending-snapshot fixtures; 50 repeated runs each | pass | Cancellation is observed before barrier release and cleanup converges. |
| Close-responsive pending replay | D3 interrupted replay close/retry fixture; 50 repeated runs | pass | The retry receives the exact missing range and then the live tail. |
| Bounded ordered request processing | `TUI_REQUEST_QUEUE_CAPACITY`, one `ws_rx` owner, retained request handler, lifecycle guard | pass | Queue saturation is fail-closed; no per-message detached task is used. |
| Complete task ownership | Exact probe counters, drop/completion guards, `all_handles_consumed_for_test` | pass | Writer, socket reader, handler, raw task, and forwarders are joined. |
| Forwarder cleanup | Installed and joined forwarder counters in rollback helper | pass | Every closure fixture asserts installed == joined. |
| Canonical critical delivery | `observed_and_unobserved_critical_send_have_identical_terminal_results` and source guard | pass | Observer-enabled and disabled paths share the production future and one budget. |
| Queue causal evidence | `real_core_full_queue_operation_correlated_timeout` and `real_tui_full_queue_operation_correlated_timeout` | pass | Fullness is established before the target operation and the target result is correlated. |
| Real TUI ownership rollback | D1–D4 helper assertions | pass | Actual staged subscription IDs, receiver non-reuse, daemon baseline, and idempotent unsubscribe are checked. |
| Typed Unix I/O evidence | F0–F6 production socket observer fixtures | pass | Narrow platform error kinds are asserted from the real write path. |
| Replay durability after failure | D3 and F4 retry fixtures | pass | History remains durable and exact replay-to-live continuity is preserved. |
| Probe registration | Identity-keyed `std::sync::Mutex` registry with bounded retention | pass | Registration is infallible and correlated by actual connection ID. |
| Non-interference | Core/TUI continuity fixtures and unrelated-client marker assertions | pass | A failing connection does not disrupt another client. |
| Planning integrity | This record, M012 plan, M011 conditional record, roadmap, and registry | pass | Statuses and final code-head references agree. |

## 3. Production implementation evidence

### Work Package A — TUI reader/handler lifecycle

`upgrade_tui` now transfers the WebSocket read half to exactly one socket-reader task. Text frames are parsed and sent through a bounded `mpsc` queue to a separately retained sequential request-handler task. Close frames, EOF, read errors, queue closure, and queue saturation all reach the common cancellation path. The handler selects on connection cancellation while pending, including during critical delivery. The connection owner retains and joins the reader and handler alongside the existing writer and raw-event tasks.

### Work Package B — Task ownership and probes

`ConnectionTaskSet` owns the TUI request handler explicitly and records its completion separately from cleanup. Per-connection probes record exact send, reader, handler, raw-event, cleanup, cancellation, and forwarder lifecycle data. `ConnectionProbeRegistry` uses an identity-keyed, poison-tolerant standard mutex with bounded finalized retention; registration no longer uses fallible `try_lock()` or insertion-order correlation.

### Work Package C — Canonical critical delivery

Observed and unobserved critical sends now call the same canonical staged-delivery future. The one total delivery budget spans queue enqueue and writer receipt. Stage flags are set at their actual boundaries, queue capacity uses the channel maximum, and remaining capacity is sampled immediately before send. The final typed result is correlated to the operation rather than inferred from connection-wide history.

### Work Package D/E — TUI interruption and rollback

D1 and D2 prove graceful and abrupt pending-snapshot cancellation. D3 proves interrupted replay rollback and exact retry. D4 runs 50 alternating cycles in one test. Rollback checks actual client/connection ownership, daemon subscription baseline, receiver non-reuse, duplicate unsubscribe, exact task counts, forwarder joins, queue/sender release, probe finalization, and no post-failure projection leakage.

### Work Package F — Unix typed I/O

The Unix daemon socket transport records connection ID, operation ID, boundary, write/flush progress, error kind, and terminal result directly from the production write path. F0 is the successful-write control. F1, F2, and F4 establish peer closure before the server write, wait for the typed observation, and retain EOF/rollback only as convergence evidence. F6 repeats typed peer-error recovery over 25 internal cycles per invocation.

### Work Package G/H — Join evidence and guards

The task-owner test seam now exposes consumed-handle state and direct completion/drop evidence instead of relying on elapsed sibling sleep time. The lifecycle guard rejects the known false-positive shapes: unbounded or detached TUI handling, duplicate observed-send implementations, capacity conflation, weak rollback identity, fallible probe registration, and untyped Unix evidence.

## 4. Verification executed

### Commands run

Formatting, build, and lint:

```bash
cargo fmt -- --check
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
CARGO_BUILD_JOBS=1 cargo clippy -p codegg-protocol --all-targets -- -D warnings
CARGO_BUILD_JOBS=1 cargo clippy -p codegg --lib --all-features -- -D warnings
```

Focused codegg verification:

```bash
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib server::ws --all-features -- --nocapture
CARGO_BUILD_JOBS=1 cargo test -p codegg --lib core::transport::daemon_socket -- --test-threads=1 --nocapture
CARGO_BUILD_JOBS=1 cargo test -p codegg --test projection_transport_real --features server -- --test-threads=1
```

Focused regression matrix:

```bash
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
cargo test --test single_daemon_lifecycle -- --test-threads=1
```

Static and boundary guards:

```bash
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

Stability commands included focused 50-run loops for each D1, D2, and D3 fixture; a 25-run loop of the full `projection_transport_real` binary; and a 25-invocation loop of the typed Unix F6 recovery fixture. The D4 fixture itself executes 50 alternating graceful-close and abrupt-drop cycles in one run.

### Results

- `cargo fmt -- --check`: pass.
- Workspace check: pass, 0 errors and 4 pre-existing warnings in unrelated TUI persistence/application code.
- Protocol clippy: pass with `-D warnings`.
- `server::ws` unit target: 9 passed.
- `core::transport::daemon_socket` unit/integration target: 33 passed.
- `projection_transport_real`: 58 passed in the final run; 25 consecutive full-binary runs passed, 1,450 test cases total.
- Replay daemon/subscription/resume/restart/isolation targets: 13/13/9/8/7 passed.
- Disclosure/artifact targets: 16/13 passed.
- TUI/render/project-routing/project-tabs/single-daemon targets: 164/99/27/20/3 passed.
- D1, D2, and D3 focused repetitions: 50/50 each, with zero failures.
- D4: 50/50 internal convergence cycles, zero failures.
- F6: 25 consecutive invocations passed; each invocation completed 25 typed Unix recovery cycles, for 625/625 internal cycles.
- All listed static and boundary guards: pass.
- `git diff --check`: pass.

The requested `codegg` clippy command remains an existing repository-wide exception: it fails on six pre-existing `codegg-core` lints in `projection_replay/artifact_registry.rs`, `artifacts.rs`, `context.rs`, `redactor.rs`, and `seam.rs`. None is in the M012 touched production surface; this is recorded as a low-severity polish/verification limitation rather than concealed as a pass.

No GitHub Actions run or attached combined check was available for this review. All evidence above is local execution and is labeled accordingly.

## 5. Invariant review

- Canonical projection DTOs, sequence meaning, cursor authority, replay persistence, and reducer semantics are unchanged.
- One task exclusively polls each WebSocket read half; TUI pending lifecycle work cannot monopolize close detection.
- Request processing is bounded, sequential, cancellation-aware, and owned by the connection.
- Connection cancellation precedes sibling teardown, and retained task handles are consumed exactly once.
- A projection subscription becomes live only after its canonical response completes successfully; failed setup/replay rolls back transient ownership without deleting durable history.
- Observer instrumentation is dormant by default and cannot alter timeout, ordering, cancellation, or error mapping.
- Socket and projection queues remain bounded; probes retain metadata only, not payloads, artifacts, reasoning, or secrets.
- Existing disclosure, artifact-handle, authentication, and compatibility boundaries remain unchanged.

## 6. Failure and recovery review

- Duplicate unsubscribe is harmless and tested against actual staged subscription IDs.
- Close, EOF, read error, queue saturation, cancellation, and writer failure converge through one connection teardown path.
- Handler cancellation while awaiting critical delivery is explicit; a parked writer is no longer required to discover peer closure.
- Forwarder cancellation is awaited and installed/joined counts are compared.
- Interrupted replay does not delete history; a fresh connection retries the exact missing range and then receives the next live sequence.
- Task panics classify the first terminal task while still cancelling and joining siblings.
- D4 and F6 show repeated baseline convergence without retained subscriptions, probes, queues, or forwarders.
- No restart, migration, schema, retention, or cursor-authority behavior changed in this milestone.

## 7. Migration and compatibility review

No database schema, projection DTO, protocol version, replay retention, cursor, or sequence migration was introduced. The new reader/handler split is transport-internal and preserves message ordering. Existing `/core`, `/tui`, and Unix compatibility behavior remains covered by the prior regression matrix. Test-only observers and seams are dormant unless explicitly configured.

## 8. Security review

The connection queue is explicitly bounded, preventing an unbounded per-message workload. Probe and observer records are payload-free and do not retain secret material. Existing policy, disclosure, redaction, artifact, authentication, and authorization boundaries are unchanged. The test-only unique sentinel fix prevents concurrent test records from colliding; it does not alter production secret handling.

## 9. Documentation and operations

Updated or verified:

- `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md` — closed status.
- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md` — conditional historical status.
- `plans/subsystems/session-projections-roadmap.md` — M012 and subsystem closed.
- `plans/registry.md` — active/dependency-ready/blocked sections reconciled and M012 recorded under recently closed work.
- `scripts/check_projection_transport_lifecycle.py` — semantic M012 guard coverage.

Operationally, a future transport regression should run the focused TUI D1–D4 and Unix F0–F6 targets first, then the full `projection_transport_real` binary under the repository's capped Cargo policy. Local output must continue to be labeled local unless CI evidence is attached.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `cargo clippy -p codegg --lib --all-features -- -D warnings` reports six pre-existing `codegg-core` lints outside M012 scope | Repository-wide warning cleanliness remains incomplete; no M012 correctness or security impact | Track as unrelated lint/polish cleanup; do not reopen M012. |

There are no unresolved critical, high, or medium M012 findings. The M011 high/medium findings are resolved by the accepted M012 implementation and evidence above.

## 11. Roadmap disposition

Milestone closed and the Session Projections subsystem may remain at strict closed status. M011 remains conditionally closed as a historical corrective record; M012 is the accepted final closure authority.

The registry audit found no registered future implementation plan whose hard or interface dependency is M012. No future plan status required changing and no new dependency-ready plan was created. Deferred product work listed in the registry remains intentionally unregistered and is not considered unblocked correctness work.

## 12. Registry updates

The following reconciliation is included with this closure:

- M012 source plan is marked closed and links this record.
- M011 source plan remains conditionally closed and is superseded for strict closure by M012.
- The Session Projections roadmap is marked closed; M012 is closed in its milestone table.
- M012 is removed from dependency-ready and active-closure sections.
- The blocked-work section is empty because no registered milestone remains blocked by M012.
- M012 is recorded under recently closed work with implementation and final reviewed code commits.
- The dependency audit found no future registered plan to unblock; deferred product work remains outside the registry's active handoff.

## 13. C1–C18 closure criteria

| ID | Result | Evidence |
|---|---|---|
| C1 | pass | D1 and D2 cancel before barrier release; each passes 50 repeated focused runs. |
| C2 | pass | D3 closes pending replay, rolls back, retries the same cursor, and passes 50 repeated focused runs. |
| C3 | pass | One reader owns `ws_rx`; bounded `TUI_REQUEST_QUEUE_CAPACITY`; separately retained sequential handler; lifecycle guard passes. |
| C4 | pass | Exact per-kind probe/drop assertions and consumed-handle checks cover writer, reader, handler, raw task, and owner teardown. |
| C5 | pass | Every closure helper asserts installed projection forwarders equal joined forwarders. |
| C6 | pass | Canonical observed/unobserved parity unit test and lifecycle source guard; one total staged-delivery budget. |
| C7 | pass | `max_capacity()` and current capacity are recorded separately; enqueue and receipt flags are set at their actual boundaries. |
| C8 | pass | Core and TUI operation-correlated full-queue fixtures establish fullness before the target request and assert the target timeout. |
| C9 | pass | TUI rollback captures actual staged IDs, checks daemon baseline/receiver non-reuse, and verifies duplicate unsubscribe. |
| C10 | pass | F0–F6 observe production socket writes and assert narrow typed `io::ErrorKind` outcomes; EOF is only convergence evidence. |
| C11 | pass | F4 and D3 interrupt delivery, preserve replay, and prove exact retry/live continuity. |
| C12 | pass | Mutex-backed identity registry handles poisoned locks, does not use `try_lock()`, retains bounded finalized records, and correlates by connection ID. |
| C13 | pass | Core/TUI two-client continuity and marker assertions show unrelated clients remain live. |
| C14 | pass | D1–D4 and Unix rollback helpers assert exact tasks, handler, forwarders, ownership, receiver, queues, probes, and subscriptions. |
| C15 | pass | D1/D2/D3 are 50/50; full binary is 25/25; F6 is 25/25 invocations with 625/625 internal cycles; D4 is 50/50. |
| C16 | pass | `check_projection_transport_lifecycle.py` passes with bounded-reader, canonical-send, rollback, identity, registration, and typed-I/O checks. |
| C17 | pass | Plan, M011 conditional record, M012 closure, roadmap, registry, code commits, counts, exceptions, and local-only CI status agree. |
| C18 | pass | Source audit and all closure evidence show no unresolved high or medium M012 finding. |

