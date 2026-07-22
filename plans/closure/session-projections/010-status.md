# Session Projections Milestone 010 — Closure Status

Status: closed (strictly superseded by `plans/closure/session-projections/011-status.md`)

Source implementation plan:

- `plans/implementation/session-projections/010-mechanism-faithful-transport-verification-and-final-closure.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md`

Repository baseline reviewed: `8bd59b22662a289f3124c9b3113e545faa9446d7`

Implementation and evidence commits:

- `a3ab136868236ff56ec221813c3da9f299993967` — main M010 instrumentation, WebSocket/Unix fixtures, guards, closure, roadmap, and registry changes.
- `7e31d573e4b02334751ce0fcb2ebf3c2c7614acf` — implementation-commit reconciliation.
- `0d68dca516ba1df7a59c3d55d5863381b2d6788b` — deterministic pre-`recv()` capacity-one `Full` observation and full local verification matrix.
- `e729c3abbfc45c862e6636d29a3ea9d64e5c28a9` — closure record of an observed projection transport timing flake.
- `131adaac6941f9276d7dd9c96cb2e086dee1f4d8` — probe-completion scheduling-flake correction.
- `8bd59b22662a289f3124c9b3113e545faa9446d7` — closure evidence updated after repeated clean runs.

Strict-verification follow-up:

- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

## 1. Closure decision

M010 materially improved the projection transport verification surface and did not reveal a new protocol, storage, reducer, disclosure, or production transport architecture defect.

The production-safe test controls and useful fixtures remain accepted. Strict closure is invalidated because several tests and guards do not establish the exact causal mechanism claimed, the rollback evidence is incomplete and sometimes aggregate, and documentation overstates resolution.

M011 must close these evidence defects before the session-projections subsystem returns to strict closed status.

## 2. Accepted M010 outcomes

### 2.1 Connection-local controls

Accepted additions include:

- configurable bounded outbound queue capacity;
- writer gates before receive and before item delivery;
- transport lifecycle observer and outbound sender access;
- first terminal task-kind and panic classification recording;
- raw-source cancellation control;
- dormant production default with no test configuration installed.

### 2.2 Capacity-one observation

`real_core_outbound_queue_capacity_is_one_when_configured` validly proves that a capacity-one outbound channel can be held before `recv()`, filled once, and observed as `Full` by a second nonblocking send.

This is retained as a queue-boundary primitive. It is not by itself proof that a lifecycle response's production `tx.send()` timed out while the queue was full.

### 2.3 TUI pending-delivery interruption

The TUI snapshot and replay writer-barrier fixtures validly improve evidence that a real peer can disconnect before canonical response delivery completes and that replay remains durable for a later connection.

These tests remain cancellation/interruption evidence. They are not substitutes for a TUI full-queue timeout fixture.

### 2.4 Raw-source and task observations

The `/core` raw-source cancellation fixture validly triggers a connection-local raw task and observes `ConnectionTaskKind::RawEvent` before normal peer close.

First-task-kind recording in the production task owner remains accepted. Panic classification coverage is useful but does not yet constitute the required six-case production cancel/abort-and-await matrix.

### 2.5 Unix coverage

The M010 Unix tests validly add:

- disconnect cleanup after an active subscription;
- lifecycle-seam writer-failure recovery;
- listener-shutdown cleanup;
- normal retry replay after a prior disconnect;
- distinct Unix client and subscription identities.

These are retained as regression coverage but are not mechanism-faithful proof of every M010 Unix acceptance criterion.

### 2.6 Verification and flake correction

The recorded focused local matrix and static guards remain historical evidence. The later scheduling fix replaced tight `yield_now()` loops with bounded sleeps and reportedly produced ten consecutive passing projection transport runs.

No GitHub workflow runs or combined status checks were attached to the reviewed final head. Local execution must remain labeled local evidence.

## 3. Post-closure findings

### 3.1 `/core` timeout is not operation-correlated

The queue-timeout fixture sends the target request before inserting its filler. The writer may already have received the target response, leaving the production operation waiting for writer receipt rather than waiting for channel capacity.

The fixture then accepts any timeout found in connection-wide send-result history. It does not prove:

- the queue was full before the target `tx.send()` began;
- the target enqueue never completed;
- timeout occurred during reservation rather than receipt wait;
- the timeout belongs to the intended request rather than another send.

M011 must establish fullness first, then start the target request, and assert an operation-correlated enqueue-stage timeout.

### 3.2 `/tui` full-queue timeout is absent

M010 added TUI writer-barrier interruption, not actual queue saturation. No TUI fixture observes `TrySendError::Full` before a real lifecycle request and then directly attributes `CriticalDeliveryError::Timeout` to that request's blocked enqueue.

M011 must add the symmetric TUI full-queue fixture.

### 3.3 Six-case task-owner teardown is incomplete

The new matrix covers three panic classifications through a helper that mirrors selection, aborts siblings, and returns without awaiting their terminal results. It does not prove production cancellation, sibling joins, handle clearing, or cleanup for:

- send completes first;
- receive completes first;
- raw-event completes first;
- send panics first;
- receive panics first;
- raw-event panics first.

M011 must run all six through the actual production teardown path or an unchanged private wrapper around it.

### 3.4 `/tui` raw-source-first is not implemented

The retained TUI test publishes an event and then closes the client. It proves peer-close teardown, not raw-source termination as first exit.

M011 must trigger the same connection-local raw-source control for TUI while the peer remains healthy and assert `RawEvent` first.

### 3.5 Unix mechanism matrix remains incomplete

The M010 Unix fixtures do not prove all named mechanisms:

- peer close occurs after the subscription is already active, not before canonical response completion;
- writer failure uses `fail_next(DuringWriterWrite, WriterClosed)` rather than a real peer-induced I/O failure;
- listener shutdown is not a barrier-controlled response-completion race with both terminal orders;
- replay retry does not interrupt the second resumed connection during replay response delivery;
- repeated race/churn convergence is absent.

M011 must add actual pre-write peer failure, real I/O error observation, forced completion-first/cancellation-first outcomes, second-connection replay interruption, and repeated convergence.

### 3.6 Rollback evidence is incomplete and aggregate

The helper currently checks daemon subscription baseline, aggregate task counters, receiver non-reuse, duplicate unsubscribe, and final active count.

It does not prove:

- failed connection ownership removal;
- projection-forwarder joins;
- handler completion;
- no canonical/live leakage;
- unrelated-client continuity;
- bounded queue/retry/resource counters;
- per-connection exact task counts when one probe is reused by multiple connections.

Some M010 fixtures use bespoke cleanup assertions instead of the helper.

M011 must provide per-connection probes and one complete rollback/non-interference harness used by every closure-relevant real failure fixture.

### 3.7 Static guards remain weakly semantic

Current guards mostly require function names and selected substrings. They do not reject:

- request-before-full queue ordering;
- connection-wide `any_timeout()` attribution;
- missing TUI saturation;
- abort-without-await test helpers;
- injected Unix writer failure;
- normal replay retry mislabeled as interrupted replay;
- incomplete rollback assertions.

M011 must add ordering and mechanism checks for these exact false-positive patterns.

### 3.8 Planning and evidence were inconsistent

M010 plan remained marked ready, closure contained a placeholder commit, registry omitted the final reviewed head, and strict closure was reported without GitHub checks.

This record corrects M010 to conditional status. M011 owns final exact reconciliation.

## 4. Unresolved findings

| Severity | Finding | Impact | Required M011 action |
|---|---|---|---|
| medium | `/core` timeout can be writer-receipt timeout rather than full-queue enqueue timeout | Queue mechanism attribution remains unproven | Establish fullness before request and record target operation stage/result |
| medium | `/tui` full-queue timeout is absent | One WebSocket adapter lacks required queue evidence | Add symmetric operation-correlated TUI saturation fixture |
| medium | Six-case task-owner matrix does not execute production joins | Panic/first-exit cleanup guarantee remains incomplete | Run six deterministic cases through production teardown and assert joins |
| medium | TUI raw-source-first remains peer-close coverage | Adapter symmetry is incomplete | Add controlled TUI raw-source termination and first-kind assertion |
| medium | Unix pre-response I/O, race, and replay-interruption mechanisms remain unproven | Unix strict lifecycle/replay closure is unsupported | Add real peer-induced I/O and forced race/replay fixtures |
| medium | Rollback harness omits required per-connection invariants | Cleanup and non-interference evidence remains fragmented | Add per-connection probes and complete harness used everywhere |
| low | Static guards permit M010 false-positive patterns | Regression protection is insufficient | Add causal-order and mechanism assertions |
| low | Closure/registry/CI evidence was inconsistent | Auditability is reduced | Reconcile exact commits, counts, commands, and absence of CI |

## 5. Roadmap disposition

M010 is strictly superseded by `plans/closure/session-projections/011-status.md`. The accepted M010 instrumentation (`ConnectionTaskSet`, `ConnectionTaskProbe`, `WriterGate`, `TransportLifecycleObserver`), TUI interruption, `/core` raw-source, capacity-one, test-matrix, and flake-fix outcomes are retained and incorporated into M011 closure evidence. No session-projection plan may be marked dependency-ready except through a future milestone that reopens the M011 evidence correctness or mechanism verification chain.

The M010 closure evidence is checked by the M011-extended `scripts/check_projection_transport_lifecycle.py` static guard (`check_projection_transport_lifecycle.py`).

## 6. M010 fixtures preserved for closure audit

- `real_core_queue_saturation_observer_records_timeout` — capacity-one `Full` observation via real seam pause.
- `real_core_connection_task_owner_first_exit_classifies_panic_per_kind` — three-kind panic classification matrix (later superseded by M011 six-case matrix).
- `real_core_outbound_queue_capacity_is_one_when_configured` — outbound queue capacity set to one and observed full by the second `tx.send()`.
- `socket_peer_close_during_writer_delivery_removes_subscription_and_eofs` — Unix peer close during writer delivery with rolled-back subscription.
- `socket_interrupted_replay_retry_resumes_with_fresh_identity` — Unix interrupted replay retry with fresh identity.
- `socket_consecutive_subscriptions_yield_distinct_identities_and_isolation` — Unix fresh-identity proof and isolation.

These remain auditable evidence; M011 added F1–F5 to satisfy the M011 mechanism-faithful acceptance criteria.