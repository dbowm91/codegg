# Session Projections Milestone 010 — Mechanism-Faithful Transport Verification and Final Closure

Status: conditionally closed — superseded for strict evidence by Milestone 011

Repository baseline: `426dfffec05c9d694f54a816213a6cca514e91b4` (`main`)

Source subsystem:

- `plans/subsystems/session-projections-roadmap.md`

Conditional closure:

- `plans/closure/session-projections/010-status.md`

Strict-evidence follow-up:

- `plans/implementation/session-projections/011-evidence-correctness-and-mechanism-verification-closure.md`

Primary class: verification correction / bounded-queue mechanics / Unix transport races / lifecycle observability / closure reconciliation

## 1. Historical objective

M010 was intended to replace nominal M009 queue, task-lifecycle, TUI interruption, Unix lifecycle, rollback, guard, and closure claims with direct production-mechanism evidence.

The production implementation and verification controls remain accepted. Post-implementation review found that several closure claims still exceed what the fixtures causally prove. M011 owns only the remaining evidence and verification correction.

## 2. Accepted M010 outcomes

M010 validly delivered:

- `ProjectionTransportTestConfig` with connection-local outbound-capacity override, writer gate, raw-source cancellation control, and lifecycle observer;
- a deterministic pre-`recv()` writer gate that allows a capacity-one channel to be observed as full;
- `TransportLifecycleObserver` send-result history and outbound sender access;
- `ConnectionTaskProbe` first-task-kind and panic classification fields;
- production `ConnectionTaskSet::join_after_first_exit` first-kind recording;
- `/core` controlled raw-source cancellation with `RawEvent` first-exit evidence;
- TUI pending snapshot and replay interruption through a writer barrier;
- broader Unix peer-close, shutdown, identity, replay, and recovery coverage;
- a focused local verification matrix;
- scheduling-flake correction and ten consecutive passing `projection_transport_real` runs.

Implementation/evidence commits:

- `a3ab136868236ff56ec221813c3da9f299993967` — main M010 implementation and initial closure;
- `7e31d573e4b02334751ce0fcb2ebf3c2c7614acf` — commit reconciliation;
- `0d68dca516ba1df7a59c3d55d5863381b2d6788b` — deterministic pre-`recv()` capacity-one `Full` observation and full local matrix;
- `e729c3abbfc45c862e6636d29a3ea9d64e5c28a9` — closure record of the observed timing flake;
- `131adaac6941f9276d7dd9c96cb2e086dee1f4d8` — probe-completion flake correction;
- `8bd59b22662a289f3124c9b3113e545faa9446d7` — closure evidence updated after the flake fix.

## 3. Residual evidence findings

M010 is not failed and its production changes are not reverted. Strict closure is deferred because:

1. `/core` queue timeout is not operation-correlated. The filler is inserted after the target request, so the observed timeout can occur while awaiting writer receipt rather than while reserving capacity on a full channel.
2. `/tui` has pending-response interruption coverage but no actual full-queue critical-send timeout fixture.
3. The claimed six-case task matrix contains three panic classifications through a helper that aborts siblings without awaiting them; clean send/receive/raw first-exit production-path cases remain absent.
4. `/tui` raw-source-first remains a peer-close fixture rather than controlled raw-source termination.
5. Unix tests do not yet prove peer close before canonical response completion, actual peer-induced production I/O failure, deterministic completion-versus-cancellation orders, or interruption of the resumed replay response.
6. The rollback helper omits direct connection ownership, projection-forwarder joins, handler completion, no-live-leakage, unrelated-client continuity, and bounded resource counters; it is not used by every real failure fixture.
7. Connection probes may be shared across multiple upgraded connections, making aggregate counts weaker than per-connection evidence.
8. Static guards remain primarily fixture-name and substring checks.
9. Closure evidence contained a placeholder commit and the registry did not name the final reviewed head or absence of GitHub checks precisely.

## 4. M011 ownership

Milestone 011 exclusively owns:

- per-connection probe ownership;
- operation-correlated queue evidence;
- deterministic `/core` and `/tui` full-queue timeout fixtures;
- six production-path task-owner first-exit cases;
- real `/tui` raw-source-first coverage;
- actual Unix pre-response I/O and replay-interruption races;
- complete rollback/non-interference assertions;
- semantic static guards;
- exact final closure evidence.

## 5. Disposition

M010 remains conditionally closed. It may return to historical resolved status only after an accepted `plans/closure/session-projections/011-status.md` record demonstrates every M011 acceptance criterion.