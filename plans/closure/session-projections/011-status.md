# Session Projections Milestone 011 — Closure Status

Status: conditionally closed — Milestone 012 TUI disconnect lifecycle and final evidence closure required

Source implementation plan:

- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Strict-closure follow-up:

- `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md`

Repository baseline reviewed: `8bd59b22662a289f3124c9b3113e545faa9446d7`

Final post-M011 head reviewed: `1a93167ee3bdfdc55e4bd2746180443cc19b7c96`

Implementation, follow-up, and evidence commits:

- `560b8b7f95c101f2e3b08a940084a94c166e80fb` — per-connection probes, operation-correlated queue observations, `/core` and `/tui` saturation fixtures, production-path task-owner matrix, and raw-source controls.
- `ae0a53f2259316217cdd55a7a158e2500a874f75` — Unix F1–F5 fixtures and writer-gate/cancellation corrections.
- `b98a626217505f094a210a8c640e1f1530280cfb` — rollback-helper expansion and fixture adoption.
- `0b61fbd97e41dca899a4626f81be420e018b1233` — M011 lifecycle guard expansion.
- `226393c08fd0035e309752c3acd0af97373d78c4` — original M011 closure reconciliation.
- `573d8885c8638a617fe1eac08c5c05853a153224` — guard placeholder refinement.
- `8f3da16f095e25123714eea771a79bd20f62aa5b` — closure commit reconciliation.
- `b868e082eb41ca4d3b69b6a5eb360cb9e18b3d3a` — final guard-refinement commit reconciliation.
- `93c3549e2c5d2481cb6422c84b6c7bb3dc7c0e50` — TUI operation-correlated projection response wiring.
- `9642bad8c0783c712bbd73d466a5f05f47ad642e` — Work Package C closure record correction.
- `11c3b42ffc913bb66504cebd939c3685f8eb028b` — TUI writer cancellation arm added for `/core` parity.
- `1a93167ee3bdfdc55e4bd2746180443cc19b7c96` — documented the remaining TUI deadlock and partial mitigation.

## 1. Closure decision

M011 materially improved Session Projections verification and retains substantial accepted value. It does not support strict closure.

The final reviewed head documents a reproducible production lifecycle defect in `/tui`: while the receive task is awaiting an inline projection handler, it cannot poll `ws_rx` for peer close. If the writer is also parked and no other task fires `connection_cancel`, the connection task owner never receives a first terminal task, joined teardown does not begin, and the daemon-side projection subscription can leak.

Adding the writer-side cancellation branch reduced the observed pending-snapshot failure rate from approximately 40–50% to approximately 20%, but did not remove the deadlock. A reduced flake rate is not closure evidence.

Post-closure source review also found that several M011 evidence claims remain broader than the mechanisms asserted. M012 is the sole authority for the production lifecycle correction and final strict closure.

## 2. Accepted M011 outcomes

The following work remains accepted and should not be reverted without a directly demonstrated defect.

### 2.1 Per-connection and lifecycle instrumentation

- `ConnectionProbeFactory` and `ConnectionProbeRegistry` introduced per-connection probe allocation.
- `ConnectionTaskProbe` records first task kind, panic classification, task completions, cleanup, and a projection-forwarder counter.
- `ProjectionTransportTestConfig` retains bounded queue capacity, writer gates, raw-source cancellation, and observer controls.
- Production defaults remain dormant when no test configuration is installed.

### 2.2 Queue preconditions and adapter symmetry

- `/core` and `/tui` capacity-one fixtures establish queue fullness before the target lifecycle request.
- Both fixtures use operation-correlated observations rather than `any_timeout()` as their primary assertion.
- TUI projection subscribe/resume/ack response paths can record observations when configured.

These fixtures remain useful, but M012 must unify the observer and non-observer critical-send implementation so tests execute the exact one-budget production semantics.

### 2.3 Task-owner and raw-source coverage

- Six clean/panic first-exit cases invoke the production task-owner teardown wrapper.
- `/core` and `/tui` raw-source cancellation fixtures observe `RawEvent` as first task while the peer remains healthy.
- Writer-gate cancellation handling was corrected to return when cancellation fires.

M012 must replace elapsed-time sibling-join proof with direct completion/drop evidence and extend ownership for the new TUI request-handler task.

### 2.4 Unix regression coverage

M011 added useful Unix coverage for:

- peer close around canonical response delivery;
- write-path recovery scenarios;
- completion-versus-cancellation ordering;
- interrupted replay and retry;
- repeated convergence and fresh identities.

These remain regression coverage. M012 must add a typed production socket-write observer and assert the actual server-side I/O result rather than inferring it from EOF and cleanup.

### 2.5 Local verification and guard work

- M011 recorded focused local test runs and repeated Unix cycles.
- `scripts/check_projection_transport_lifecycle.py` was expanded with M011-specific checks.
- No GitHub workflow runs or combined status checks were attached to the reviewed head; all recorded execution remains local evidence.

## 3. Unresolved findings

### 3.1 High — TUI pending-handler disconnect deadlock

The `/tui` receive task owns `ws_rx` and awaits `handle_tui_message_with_observer` inline. While a projection response is pending, it cannot observe Close, EOF, or socket error. The writer cancellation arm only helps after some other task fires the cancellation token.

Observed consequence:

- connection teardown can hang;
- cleanup can fail to execute;
- the daemon-side subscription can remain active;
- `real_tui_pending_snapshot_interruption_via_writer_barrier` remains flaky at approximately 2 failures per 10 runs after mitigation.

Required M012 resolution:

- retain a close-responsive socket-reader task while ordered lifecycle handling is pending;
- use a bounded request pipeline;
- explicitly own and join reader, handler, writer, raw-event, and projection-forwarder tasks;
- demonstrate 0 failures in the required repeated runs.

### 3.2 Medium — observed critical delivery is a parallel implementation

Observer-enabled staged delivery uses a separate implementation and applies independent bounded waits to enqueue and receipt stages. Normal staged delivery uses one total bounded-delivery budget.

Required M012 resolution:

- one canonical implementation;
- optional in-place observation;
- one total timeout budget;
- parity tests for observer enabled/disabled outcomes;
- correct maximum-capacity versus remaining-capacity metadata;
- correct enqueue-completed state even when receipt later fails.

### 3.3 Medium — Unix typed I/O result is not asserted

Unix F1/F2 assert EOF and subscription rollback but do not capture the server-side `std::io::Error` result or error kind. Listener shutdown/cancellation can compete with the claimed peer-write failure.

Required M012 resolution:

- peer read-side/full-peer close before server write;
- no listener shutdown as a competing cause before observation;
- typed observation from the production write path;
- narrow accepted `io::ErrorKind` assertions;
- EOF and cleanup retained only as convergence evidence.

### 3.4 Medium — rollback is not complete

Current helpers do not assert the existing projection-forwarder counter. They also do not fully prove connection ownership removal, outbound sender release, no canonical/live leakage, or exact probe-registry convergence.

The TUI helper incorrectly claims TUI has no daemon-side projection subscription even though the adapter creates and unsubscribes daemon-issued projection subscriptions.

The full-queue core fixture uses a synthetic subscription ID rather than the real staged identity.

Required M012 resolution:

- actual staged subscription identity;
- daemon and connection ownership baselines;
- receiver non-reuse;
- installed-versus-joined projection-forwarder equality;
- exact task/handler/cleanup counts;
- no response/live leakage;
- unrelated-client marker delivery;
- no synthetic IDs.

### 3.5 Medium — probe registration can be silently lost

`ConnectionProbeRegistry::factory()` uses `try_lock()` and ignores registration failure. Concurrent upgrades or registry reads can therefore lose correlation records.

Required M012 resolution:

- infallible registration;
- correlation by actual connection identity or explicit sequence;
- bounded finalized records;
- exact removal/finalization behavior.

### 3.6 Low — sibling-join timing test is not reliable proof

The test expects teardown to wait for sleeping siblings, but production aborts siblings before awaiting them. Aborted Tokio sleeps should normally resolve promptly.

Required M012 resolution:

- direct drop/completion guards;
- consumed handle assertions;
- exact probe counts;
- no elapsed-time assumption.

### 3.7 Low — guards and planning state remain inconsistent

The M011 implementation plan remains historically marked ready, while the original closure, roadmap, and registry claimed strict closure despite documenting a production deadlock and residual failures.

This corrected record makes M011 conditional. M012 owns final reconciliation of plan status, roadmap, registry, exact commits, stability evidence, and CI truthfulness.

## 4. Unresolved findings table

| Severity | Finding | Impact | Required M012 action |
|---|---|---|---|
| high | TUI cannot observe peer close while inline handler is pending | Deadlock and subscription leak remain possible | Split close-responsive reader from bounded ordered handler and prove clean repeated convergence |
| medium | Observer-enabled staged send changes timeout semantics | Queue tests do not execute exact production behavior | Use one canonical one-budget implementation with optional observation |
| medium | Unix I/O failure is inferred, not typed | Claimed production write error remains unproven | Add typed production socket-write observation and real peer read-side/full close |
| medium | Rollback omits forwarder/ownership/leakage assertions | Complete cleanup remains unsupported | Use real subscription identity and assert all tasks, forwarders, ownership, receiver, queues, and unrelated client |
| medium | Probe registration can silently fail | Per-connection evidence may be missing or miscorrelated | Replace `try_lock()` registration with infallible identity-keyed mechanism |
| low | Join proof relies on elapsed time | Abort-and-await evidence is fragile | Assert task guards, handle consumption, and exact completions |
| low | Closure documents contradict residual findings | Auditability and handoff state are incorrect | M012 exact closure, roadmap, registry, and CI/local reconciliation |

## 5. Roadmap disposition

M011 remains conditionally closed. Its accepted implementation is retained as the M012 foundation.

M012 is the sole dependency-ready Session Projections plan:

- `plans/implementation/session-projections/012-tui-disconnect-lifecycle-and-final-evidence-closure.md`

The subsystem may return to strict closed status only through an accepted:

- `plans/closure/session-projections/012-status.md`

That record must satisfy every explicit M012 closure criterion and contain no unresolved high or medium finding.
