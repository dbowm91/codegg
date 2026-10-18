# User Prompt Queue and Turn Contention Milestone 003 — Queue Delivery Through Turn Admission

Status: **closed** — see `plans/closure/user-prompt-queue/003-status.md`

Authority: `plans/adrs/ADR-0038-queue-delivery-execution-authority.md`
(**accepted** 2026-10-18, selecting **Option B**). The ADR-0030/ADR-0034
precedent blocked this plan while that ADR was `proposed`; accepting it
discharged the blocker. **Option B** means: a named
`submit_queued_prompt_turn` entry, attribution reconstructed from the owner row
plus team state, **no migration**, and `run_at_ms` stays **v82** for M007.

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the M001,
M002a, and M002b trees, which are currently **uncommitted**. §3 is anchored to
the committed baseline plus M002a's v80 and M002b's v81 — re-inspect all three at
implementation time.

Source roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones`
(Milestone 3)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md`

Applicable ADRs:

- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (**accepted** —
  owns the delivery execution authority and the attributed-admission decision)
- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (**accepted** —
  owns the delivery model, the recovery rule, and the verification list)
- `plans/adrs/ADR-0037-idle-session-queue-mutation-authority.md` (**accepted** —
  owns the owner binding this milestone submits as)
- `plans/adrs/ADR-0031-authenticated-idle-continuation-delegated-turn-admission.md`
  (accepted — the daemon-internal admission precedent and its callsite guard)
- `plans/adrs/ADR-0007-shared-session-control-ownership.md` (accepted)

Primary class: **capability**

This is the first milestone in the workstream that delivers something a person
can observe. M002a and M002b were both infrastructure and both closed records
saying in those words that no user-visible capability existed. M003 is still not
a UI milestone — nothing here adds a command, a dialog, a key, or a row — but it
is the milestone where a queued prompt **actually runs**, and it must not be
described as either more or less than that.

## 1. Objective

Make a due queue entry become a real turn through the existing `TurnSubmit`
admission path, with at-most-once delivery across reconnect and daemon restart
and no replay.

This is the milestone the roadmap's first completion clause depends on: "A
prompt composed during an active turn is retained and later delivered." M001
made it retained; M002a/M002b made it storable and mutable. Nothing delivers it
yet.

## 2. Why this milestone is ready

**Ready.** M002b's hard dependency is discharged
(`plans/closure/user-prompt-queue/002b-status.md`): entries are durable, ordered,
bounded, expiring, and owned, and every mutation has a real producer. ADR-0035's
delivery model is accepted and unchanged.

**Unblocked.** This milestone inherited a decision ADR-0035 and ADR-0037 both
left open: **as whom is a queued entry submitted?** The daemon may originate an
admission, but the repository's only such path reconstructs authority from durable
attribution via `AuthenticatedPrincipal::reconstructed`, which needs five fields
(`crates/codegg-core/src/transport_auth.rs:222-236`), and the v81 owner row
stores **one** of them (`owner_principal`;
`crates/codegg-core/src/session/user_prompt_queue.rs:188`).

Delivering under the wrong identity is not a tidiness issue. Under a synthetic
daemon identity the turn controller lease is held by that identity, so the real
owner's next mid-turn queue mutation fails `user_prompt_queue_controller_mismatch`
— the queue locks itself out the moment it starts working. Using
`is_controller_bypass` instead is what ADR-0037 exists to refuse.

ADR-0038 settled that by accepting **Option B**: resolve the delivery principal
from the owner row plus `TeamStore`, classify the delivery itself rather than
reconstructing a dead connection, and take no migration.

## 3. Current implementation evidence

Beyond M002a's §3 and M002b's §3:

- **The delivery precedent already exists in production.**
  `CoreRequest::SessionPromptSubmit` (`src/core/daemon_turns.rs:791`) checks the
  receipt by `request_id` (`:798-818`), resolves composition daemon-side via
  `resolve_prompt_submit_composition` (`:824-830`), then recurses into
  `handle_turns_request(CoreRequest::TurnSubmit { .. }, request_id,
  Some(submission_digest), ..)` (`:831-849`). Delivery is the same shape with a
  deterministic `request_id` instead of a client one.
- **`resolve_prompt_submit_composition` takes no authority.** Its signature is
  `(&self, session_id, text)` (`src/core/daemon_turns.rs:2203-2214`) — no
  principal, no envelope, no request id. `src/core/daemon_continuation.rs:555-561`
  already calls it from a daemon-internal wake.
- **The daemon-internal admission precedent is ADR-0031.**
  `CoreDaemon::submit_idle_continuation_turn` (`src/core/daemon_continuation.rs:408`)
  reconstructs the principal from a durable `OriginAttribution` row (`:468-494`),
  fails closed on principal divergence (`:480-486`), re-authorizes against current
  team state (`:515-539`), and calls `handle_turns_request` directly (`:672-690`).
  `scripts/check_idle_continuation_callsites.py` restricts it to exactly two
  callers because any third would be a backdoor.
- **At-most-once is inherited, not reimplemented.** `admit_with_promotions`
  (`crates/codegg-core/src/session_control.rs:579`) is idempotent on
  `submission_id` + digest and returns `TurnAdmissionOutcome::Duplicate`
  (`:622-635`). A deterministic submission id derived from the entry id is
  therefore the whole fence.
- **The idle-transition hook exists.** `spawn_turn_reaper`
  (`src/core/daemon_control.rs:658`) is spawned per turn, subscribes
  synchronously before spawning (`:661`, readiness contract at `:645-650`), and
  on the matching terminal event does all four release steps —
  `finish_turn_receipt` (`:694`), `store.release` (`:704`), clear `active_turn`
  (`:706-712`), set `Idle` (`:714-715`). It runs **after** the lease is gone, so a
  submit from here is admissible. It clones only `sessions` and `pool`
  (`:659-660`), so calling a `&self` daemon method from inside it needs the
  `Weak<CoreDaemon>` upgrade pattern already proven at `src/core/daemon.rs:575-580`.
- **Cancellation is not an idle transition.** `TurnCancel`
  (`src/core/daemon_turns.rs:1237-1256`) only signals `cancel_tx`; it neither
  clears `active_turn` nor releases the lease. The terminal event still has to
  arrive. A submit from the cancel arm would hit `turn_already_active`.
- **`session_turn_controller` is not the only gate.** The in-memory
  `active_turn` check (`src/core/daemon_turns.rs:448-454`) precedes durable
  acquisition, and the store's conflict maps to the same code (`:538-541`).
  "Session is free" therefore means **both** `active_turn == None` **and** no
  lease row.
- **There is no periodic session observation loop.** The three
  `tokio::time::interval` loops in `src/core/` are projection retention
  (`daemon_construct.rs:166`, 300s), provider refresh (`eggpool.rs:292`, 30s,
  opt-in), and TTS drain (`notification.rs:464`, 500ms, opt-in). None observes
  sessions or turns. **M003 must therefore introduce its own trigger, and
  introducing a general poller would violate the roadmap's "no new timer thread"
  rule.**
- **Restart reconciliation has a direct precedent.**
  `reconcile_idle_continuation_wakes` (`src/core/daemon_continuation.rs:356`) is
  the bounded startup twin: capped at 512 rows, no unbounded poller, no per-job
  task, CAS-gated per row (`:382-386`). `CoreDaemon::recover_state`
  (`src/core/daemon_bootstrap.rs:196`) already calls
  `interrupt_incomplete_turn_receipts` (`:206-211`) and
  `reconcile_session_controllers` (`:277`). Startup phase order is pinned by a
  test (`bootstrap_phase_names`, `:706-713`).
- **The status vocabulary decision M003 inherited is now concrete.** v80's
  `CHECK` enumerates `pending`, `delivering`, `delivered`, `failed`, `expired`
  (`crates/codegg-core/src/session/schema.rs:3374-3376`); only `pending` and
  `expired` have producers, and SQLite cannot `ALTER` a `CHECK`. **M003 makes
  three of the five reachable, which discharges the finding**: `delivering` on
  claim, `delivered` on a recorded turn, `failed` on a recorded refusal. No
  rebuild is required, and this is the answer the roadmap asked M003 to give.
- **Three projection behaviours break the moment `delivering` exists**, and they
  are not cosmetic:
  1. `list_live` sweeps **and** selects `status = 'pending'`
     (`crates/codegg-core/src/session/user_prompt_queue.rs:320-334`). An entry
     mid-delivery silently **vanishes** from the read projection.
  2. `check_capacity` counts only `pending` (`:401-404`), so a stranded
     `delivering` row is uncounted against the backlog bound while still
     consuming the retention bound.
  3. The expiry sweep also only sweeps `pending` (`:322`), so a stranded
     `delivering` row is never expired either.
  A `delivering` row that is never resolved is therefore invisible, uncounted,
  unexpired, and never retried. Reconciling it at startup is a correctness
  requirement, not a cleanup task.
- **The frontend has no push path for the queue.** There is **no** queue
  `CoreEvent`; the queue is request/response only. `SessionState.user_prompt_queue`
  (`src/tui/app/state/session.rs:187`) is reloaded on exactly one edge — session
  change (`src/tui/app/mod.rs:12294`). Delivery is daemon-initiated with no
  response channel, so without a change the TUI would show an entry that has
  already run until the user switches sessions. `apply_core_event`
  (`src/tui/app/mod.rs:2101`) has a `_ => false` catch-all (`:2463`), so a new
  variant is additive but silently ignored without a TUI arm.
  Note also that the doc comment at `src/tui/app/state/session.rs:185-186` still
  says "Nothing writes entries yet — every mutation is M002b", which M002b made
  stale.

## 4. Invariants that must not regress

Everything in M002a §4 and M002b §4, plus:

- **A queued entry reaches execution only through ordinary `TurnSubmit`.** No
  parallel route, no second receipt namespace, no relaxation of
  interrupted-no-replay (ADR-0035).
- **Delivery is at most once**, across frontend reconnect, frontend restart, and
  daemon restart. This is inherited from the receipt, not reimplemented.
- **Nothing is replayed automatically.** An entry whose receipt is already
  `accepted` or `interrupted` is reconciled by receipt status, never resubmitted.
- **Delivery runs as the queue owner**, never as a synthetic daemon identity and
  never through `is_controller_bypass`.
- **Authorization is re-evaluated at delivery time.** A revoked, suspended, or
  capability-narrowed owner fails delivery; its entry is retained with a
  recorded reason and is never force-submitted.
- **Delivery order is the daemon-owned ordinal.** Selection is not last-writer-wins.
- **A refusal retains the entry.** Nothing drops a queued entry because it could
  not be admitted.
- **Prompt text never reaches an audit record, receipt, diagnostic, or error
  message** — and per M002b, neither does a digest of it. Only a byte count.
- **Delivery introduces no scheduler permit, executor ownership, workspace
  execution context, or new timer thread.**
- **Cancelling a turn does not discard the queue.**
- **The turn controller lease keeps its meaning for turns** (ADR-0007 unmodified).

## 5. Scope

### In scope

- Due-entry selection by daemon-owned ordinal, under transaction.
- The `pending → delivering → delivered | failed` status transitions, which make
  three of v80's five `CHECK` values reachable and discharge the inherited
  decision.
- The status-dependent projection, sweep, and capacity behaviour identified in
  §3, so a `delivering` entry is visible, counted, and expirable.
- A named daemon-internal delivery entry, restricted by its own callsite guard to
  exactly two callers — the post-terminal wake and the bounded restart twin.
- Deterministic submission identity derived from the entry id.
- Re-authorization against current team state at delivery time, per ADR-0038's
  settled clause 4 regardless of which attribution option is chosen.
- A push path so a frontend learns a delivery happened (ADR-0038 §Security and
  reliability implications requires delivery to be observable; ADR-0035's
  Verification requires the receipt and lease be observed).
- Startup reconciliation of stranded `delivering` rows by receipt status.
- The delivery `OperationDescriptor` and its `INSTRUMENTED_OPERATIONS` entry.
- A callsite-guard script for the delivery entry, with `--self-test`.

### Explicitly out of scope

- Any user-facing surface — M004/M005. No `/queue` command, no dialog, no row 0
  suffix, no composer affordance, no new key.
- Mid-turn injection and send-now — M006, still blocked on ADR-0036.
- Delayed delivery and `run_at_ms` — M007, still blocked on ADR-0036's copy
  boundary, and contending for the same migration number ADR-0038 Option C
  would take (§16).
- Whether a delivered entry is hidden or shown as a terminal row — a presentation
  question for M004. M003 records the status; it does not decide the rendering.
- Desktop parity — M008.
- A general session-observation poller or any new wakeup source.

## 6. Required production changes

### The inherited status-vocabulary decision

M003's first recorded decision, answering the roadmap's explicit question:
**make all five reachable rather than rebuild the table.** `delivering` is
written on claim, `delivered` when a turn is recorded against the entry, `failed`
when a refusal is recorded with a reason, `expired` already exists, and
`pending` is where entries wait. v80 is not edited and no rebuild is performed.

### Store — delivery transitions

- A claim operation that selects the next due `pending` entry by ordinal and
  moves it to `delivering` **in one transaction**, so two delivery attempts
  cannot claim the same entry.
- A completion operation writing `delivered` with `delivered_turn_id` and
  `delivered_at_ms` (both columns already exist in v80).
- A failure operation writing `failed` with a bounded `failure_reason`, and a
  retention path that returns a failed entry to `pending` when a caller chooses
  to retry rather than drop it.
- **Fix the three projection behaviours from §3** so `delivering` is visible in
  the read projection, counted against the backlog bound, and swept by expiry.
  Every one of these is currently a `status = 'pending'` literal that becomes
  wrong the instant `delivering` has a producer.
- Startup reconciliation of `delivering` rows against receipt status.

### Daemon — the delivery entry

- One named internal entry, guarded by its own callsite script in the shape of
  `scripts/check_idle_continuation_callsites.py`, with exactly two permitted
  callers: the post-terminal wake and the bounded restart twin.
- Authority reconstruction per ADR-0038's selected option, re-authorized against
  current team state, with the principal-divergence check ADR-0031 uses.
- A deterministic submission id derived from the entry id, so a retry replays to
  the same receipt and receives `Duplicate`.
- Submission through `handle_turns_request(CoreRequest::TurnSubmit { .. })`
  using `resolve_prompt_submit_composition` — the `SessionPromptSubmit` shape.

### Triggers

- **Post-terminal**, in `spawn_turn_reaper` after `store.release`, using the
  `Weak<CoreDaemon>` upgrade pattern at `src/core/daemon.rs:575-580` because the
  reaper clones only `sessions` and `pool`.
- **Restart twin**, bounded and CAS-gated, modelled on
  `reconcile_idle_continuation_wakes`.
- **First add while idle** is a candidate the plan deliberately leaves open: the
  user has just composed a prompt and the session is free. If it is adopted, it
  becomes a **third** callsite and the guard must say so; if not, the entry waits
  for the next terminal event or restart. Record the choice either way — an
  unrecorded third trigger is how a guard stops meaning anything.

### Projection

- A queue `CoreEvent` (or an equivalent existing refresh edge) plus a TUI apply
  arm, because delivery is daemon-initiated and the current reload edge is
  session-change only. A frontend showing a delivered entry as waiting is a
  correctness defect, not a stale-cache cosmetic.
- Correct the stale "every mutation is M002b" comment at
  `src/tui/app/state/session.rs:185-186`.

### Audit and authorization

- A delivery descriptor, `ScopeKind::ViaSession` + `Capability::AgentInvoke`,
  matching `session_control_transfer`.
- Its `INSTRUMENTED_OPERATIONS` entry mapping to `prompt_submit`, per ADR-0037's
  decision for the six mutations.
- A delivery audit record naming entry id, delivering principal, turn id, and
  outcome — with a byte count and **no text and no digest**.

## 7. Ordered work packages

### Work package A — Re-verify the authority model

ADR-0038 is `accepted` and selects **Option B**. Re-inspect v80/v81 and M002b's
store surface, since the tree is uncommitted.

### Work package B — Store delivery transitions

Claim, complete, fail, retry, and startup reconciliation, plus the three
projection fixes. Test this package at the store level before anything calls it:
a stranded `delivering` row is invisible, so a bug here is invisible everywhere
above it.

### Work package C — The delivery entry and its callsite guard

The named entry, authority reconstruction, re-authorization, deterministic
submission id, and the guard script with `--self-test`. Follow
`scripts/check_idle_continuation_callsites.py`'s structure, and prove the guard
fails on an injected third caller before citing it.

### Work package D — Triggers

Post-terminal wake and restart twin. Only these two, unless the first-add decision
in §6 was made explicitly.

### Work package E — Projection and observability

The queue event, the TUI arm, the audit record, and the stale comment fix.

### Work package F — Docs

`architecture/core.md` (the delivery contract), `architecture/session.md` (the
status model), `architecture/authorization.md` (the delivery descriptor), and the
matching skills. `docs/tui.md` **only** if a user-visible behaviour changed — it
should not have.

## 8. Failure, cancellation, restart, and contention semantics

- **Refusal to admit:** the entry returns to `pending` or becomes `failed` with a
  recorded reason. It is never dropped and never force-submitted.
- **Revoked owner at delivery time:** delivery fails, the entry is retained with
  the reason, and it is not retried in a loop.
- **Crash between claim and completion:** the row is `delivering` with no receipt;
  startup reconciles it by receipt status and returns it to `pending` or marks it
  failed. It is never left stranded.
- **Crash after receipt, before the queue write:** the receipt exists and is
  terminal; startup reconciles the queue row from the receipt rather than
  resubmitting.
- **Daemon restart:** entries never admitted remain queued; entries with an
  accepted receipt are reconciled, never replayed.
- **Two frontends racing one entry:** the claim is first-write-wins under
  transaction, so exactly one attempt delivers it.
- **Frontend reconnect:** the queue is re-read as authoritative; a local copy is
  never replayed.
- **Turn cancelled:** the queue is untouched; the terminal event arrives through
  the runtime and the reaper fires normally.
- **A failure to reach the daemon's own state** (no pool, no bound runtime):
  delivery is refused with a typed reason and the entry stays `pending`. It is not
  marked `failed` for an environmental condition that is not the entry's fault.

## 9. Compatibility and migration

- **No shipped migration is edited.** v80 and v81 are closed.
- **M003 needs no migration.** ADR-0038 selected Option B, so the delivery
  principal is resolved from the owner row plus `TeamStore` rather than from new
  columns. `STORAGE_LAYOUT_VERSION` is unchanged.
- **The v82 contest is resolved.** `run_at_ms` remains **v82** for M007 in
  `architecture/session.md` and `plans/registry.md`; neither document changes
  because of this milestone.
- Existing owner rows written under M002b need no backfill — `owner_principal`
  is resolved against team state at delivery time.
- No protocol variant changes shape. Additions are new variants, additive on the
  wire; an absent queue projection still reads as an empty queue.
- A frontend that predates the delivery event keeps its projection until its next
  reload rather than erroring — the `_ => false` catch-all already guarantees
  this, but the TUI arm is still required for correctness.

## 10. Required tests

Name every new test with a `user_prompt_queue` prefix so the §11 filters select
real tests.

### Store tests

- Claim selects by ordinal and is first-write-wins under concurrency.
- `delivering` is visible in the read projection, counted against the backlog
  bound, and swept by expiry — one test each, since each is a distinct literal
  that is currently `pending`-only.
- Complete writes `delivered` with turn id and timestamp.
- Fail writes `failed` with a bounded reason and never drops the entry.
- Retry returns `failed` to `pending` without changing identity or position.
- Startup reconciliation resolves a stranded `delivering` row by receipt status.
- All five `status` values are now reachable — the direct test that discharges the
  inherited M002a/M002b finding.

### Admission and authority tests

- Delivery reaches execution **only** through `TurnSubmit`, with the lease and
  receipt observed.
- The submission id is a pure function of the entry id.
- A retry after simulated restart returns `Duplicate` and starts no second turn.
- A **revoked** owner fails delivery and retains the entry.
- A non-owner cannot cause delivery.
- Principal divergence fails closed, as ADR-0031 requires.

### Contention and recovery tests

- Two concurrent delivery attempts converge on one delivered entry.
- Daemon restart mid-delivery never double-submits.
- A frontend reconnect re-reads rather than replays.
- Cancellation leaves the queue intact.
- `TurnCancel`/`TurnSteer`/`session_control` authorization tests pass unmodified.

### Security and negative tests

- Audit records carry entry identity and outcome, and **no text and no digest**.
- The callsite guard rejects an injected third caller, and its `--self-test`
  passes.
- No delivery path uses `is_controller_bypass`.

## 11. Required verification commands

```bash
cargo fmt

# new delivery tests (TEST-NAME filter, not a module path)
cargo test -p codegg-core user_prompt_queue

# the storage slice that delivery extends must still be green
cargo test -p codegg-core --test user_prompt_queue
cargo test -p codegg-core --test user_prompt_queue_mutation

# turn-admission authority must be unchanged by the delivery entry
cargo test -p codegg-core session_control
cargo test --test identity_m003_daemon_authorization user_prompt_queue

# startup recovery and the controller lease
cargo test --test storage_migrations
cargo test --test session_family session_control

# the new callsite guard, including its self-test
python3 scripts/check_queue_delivery_callsites.py
python3 scripts/check_queue_delivery_callsites.py --self-test

# existing guards this work must not break
python3 scripts/check_audit_coverage.py
python3 scripts/check_project_catalog_invariants.py
python3 scripts/check_project_catalog_invariants.py --self-test
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_idle_continuation_callsites.py

# lint
cargo clippy -p codegg-core --all-targets -- -D warnings
cargo clippy -p codegg --all-targets -- -D warnings

# canonical sweep
scripts/verify.sh quick
```

Every filter above must select a non-zero number of tests, or be reported as
selecting zero. Do not record a zero-select filter as a pass.

## 12. Documentation updates

- `architecture/core.md` — the delivery contract: the named entry, the two
  triggers, the deterministic submission id, re-authorization at delivery time,
  and the relation to ADR-0031's delegated admission.
- `architecture/session.md` — the status model and the answer to the inherited
  `CHECK` decision; the v82/v83 outcome if ADR-0038 selected Option C.
- `architecture/authorization.md` — the delivery descriptor and its audit
  mapping.
- `.opencode/skills/core/`, `.opencode/skills/session-storage/`, and
  `.opencode/skills/authorization/` — with their `architecture/` docs.
- `docs/tui.md` — **only** if delivery changed something a user can observe. If
  not, leave it alone and say so.

## 13. Acceptance criteria

1. A due entry becomes a real turn through ordinary `TurnSubmit`, with the
   controller lease and the receipt observed.
2. Delivery is at most once across frontend reconnect, frontend restart, and
   daemon restart, proven rather than asserted.
3. An entry interrupted mid-admission is reconciled by receipt status and never
   double-submitted.
4. A refusal retains the entry; nothing is force-submitted or silently dropped.
5. A revoked or suspended owner fails delivery and retains the entry.
6. Delivery runs as the queue owner — proven by asserting the receipt's
   `principal_id`, not by inspecting the code path.
7. All five `status` values are reachable, discharging the inherited decision
   without a rebuild.
8. A `delivering` entry is visible, counted, and expirable; a stranded one is
   reconciled at startup.
9. Delivery follows the daemon-owned ordinal, and concurrent attempts converge on
   one delivered entry.
10. The callsite guard has exactly two permitted callers, proven by an injected
    violation.
11. Audit records carry identity and outcome with no text and no digest;
    `scripts/check_audit_coverage.py` passes.
12. A frontend learns a delivery happened rather than showing a delivered entry as
    waiting.
13. No new timer thread, wakeup source, scheduler permit, or executor ownership.
14. `TurnCancel`, `TurnSteer`, and `session_control` tests pass unmodified.

## 14. Stop conditions

Stop and report rather than improvise when:

- A later decision changes ADR-0038 away from **Option B**. Its Decision section
  overrides anything here that conflicts.
- The attribution source turns out to require editing v81. A shipped migration is
  never edited; a new migration is a recorded decision — and it would contest v82.
- The first-add-while-idle trigger is proposed as a third callsite **without** an
  explicit decision to widen the guard.
- At-most-once cannot be achieved through the existing receipt without relaxing
  interrupted-no-replay. Report it; that is ADR-0035's core guarantee.
- Delivery would need a parallel submission route rather than `TurnSubmit`.
- A stranded `delivering` row proves unreconcilable without queue-specific replay
  logic, which ADR-0035 forbids.
- Scope would expand into M004, M005, M006, M007, or M008.

## 15. Closure evidence required

1. Proof ADR-0038 was `accepted`, and which option it selected, before work began.
2. The lease-and-receipt observation proving delivery used ordinary `TurnSubmit`.
3. At-most-once evidence across restart, including the duplicate receipt.
4. Receipt `principal_id` evidence proving delivery ran as the queue owner.
5. Revoked-owner retention evidence.
6. All-five-statuses reachability evidence.
7. `delivering` visibility, counting, expiry, and startup-reconciliation evidence.
8. Callsite-guard output plus the injected-violation proof and `--self-test`.
9. Audit record samples showing identity, outcome, and no text or digest.
10. `TurnCancel`/`TurnSteer`/`session_control` tests passing unmodified.
11. Guard, clippy, and `verify.sh quick` results; hosted CI explicitly not claimed
    unless run.
12. The unresolved-finding list, including any fallout on M007's `run_at_ms`.
13. An explicit statement of what user-visible capability this milestone did and
    did not deliver — the first milestone where that statement is not simply
    "none".
14. The unblock audit against the registry's Blocked work section.

## 16. Handoff notes

- **ADR-0038 is `accepted` (Option B).** The plan may start. Its Decision section
  wins over anything here that conflicts.
- **No migration is in play.** `run_at_ms` stays v82 for M007.
- **The decision is narrow.** It is about *as whom* a queued entry is submitted.
  Everything else in this milestone is settled by ADR-0035 and was not reopened
  by waiting.
- **Do not widen ADR-0031's entry.** Its guard exists to keep the
  daemon-may-submit-as-anyone surface small, and reusing it for a second ownership
  domain makes the guard less meaningful.
- **Do not reach for `is_controller_bypass` or a synthetic daemon principal.**
  Both fail correctness: the first is refused by ADR-0037, and the second locks
  the owner out of its own queue at the first mid-turn mutation.
- **A `delivering` row is the dangerous state.** It is invisible, uncounted, and
  unexpired under the code as written. Work package B exists because a bug there
  is invisible from every layer above it.
- **Preserved work.** M001's, M002a's, and M002b's trees may be uncommitted. Do
  not stash, revert, or reformat them.
- **Build cost.** `CARGO_BUILD_JOBS=2`; background long sweeps.
- **Next milestone is M004** (`/queue` box: list, select, delete, row 0), which
  needs this one closed and which needs no ADR.