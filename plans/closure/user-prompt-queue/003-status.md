# Milestone 003 — Queue Delivery Through Ordinary Turn Admission: Closure Record

Status: **closed**

Milestone: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 3, delivery slice)

Plan: `plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md`

Authority: `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` — **accepted**
2026-10-18, selecting **Option B** (a named `submit_queued_prompt_turn` entry;
attribution resolved from the owner row plus `TeamStore`; **no migration**).

Closure record: `plans/closure/user-prompt-queue/003-status.md`

Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted M001,
M002a, and M002b closure trees. Nothing in this milestone was committed; the
whole working tree remains uncommitted.

## 1. Class and delivered capability

Primary class: **capability**.

This is the **first milestone in this workstream that delivers observable
behaviour**. M001 through M002b made the queue durable, bounded, expiring,
owned, and mutable, and every one of them was still invisible: a prompt could be
queued and nothing would ever run it. M003 is where a queued prompt actually
runs.

It still adds **no UI**. There is no `/queue` command, no dialog, no key, and no
row-0 change — M004/M005 own those. What a user can observe now is behavioural,
not interactive: a queued prompt is claimed, admitted as a turn under the
owner's authority, and reflected back in the queue projection. M004 makes the
first of those states visible enough to steer.

## 2. Work package A evidence — the authority was accepted before work began

`plans/adrs/ADR-0038-queue-delivery-execution-authority.md` status line reads
`Status: **accepted**`, dated 2026-10-18, selecting **Option B**. The ADR was
authored `proposed` by the M002b closure audit, then accepted on the maintainer's
instruction before any code was written.

The milestone's status moved `blocked` → `active` as a result, in the plan, the
roadmap, and the registry. Under the ADR-0030/ADR-0034 rule a proposed ADR
confers no implementation authorization; accepting it discharged the sole
blocker.

**The v82 contest did not arise.** Option B needs no migration, so `run_at_ms`
remains **v82** for M007 and M003 takes no migration number at all. Neither
`architecture/session.md` nor `plans/registry.md` records a migration change for
this milestone.

## 3. The authority decision, as built

The delivered principal is the **queue owner**, reconstructed from durable rows
only:

| Field | Source |
|---|---|
| `principal_id` | `user_prompt_queue_owner.owner_principal` |
| `kind` | `TeamStore` — `PrincipalNotFound` fails closed |
| `auth_method` | `AuthMethod::QueueDelivery` (new) |
| `transport_class` | `TransportClass::DaemonDelivery` (new) |
| `client_id` | `queue:<entry_id>` — daemon-owned, metadata, never authority |

`QueueDelivery`/`DaemonDelivery` are a **delivery classification**, not a record
of what authenticated the owner when the prompt was queued. A delivery has no
originating connection, so `LocalOwner` or `PersonalToken` would be a
fabrication about a call that never happened. **Neither value needs a
migration**: `origin_attribution.auth_method`, `origin_attribution.transport_class`,
and `audit_event.auth_method` are unconstrained `TEXT` (verified by reading
`crates/codegg-core/src/session/schema.rs`), and `auth_method` /
`transport_class` in `turn_admission_receipt` is not persisted at all.

The accepted cost, stated plainly: audit records say **how the daemon
submitted**, not what authenticated the original caller. The entry id, the
delivering principal, the turn id, and the outcome are still recorded, and no
prompt text or digest is recorded under any option.

**Neither escape hatch is used.** `submit_queued_prompt_turn` never passes
`is_controller_bypass` and never constructs a synthetic principal;
`scripts/check_queue_delivery_callsites.py` asserts both negatives statically.

## 4. Work package B — the store delivery slice

Added to `crates/codegg-core/src/session/user_prompt_queue.rs`:

- `claim_next_for_delivery` — one due entry in ordinal order, transitioned in the
  same transaction, `AND status = 'pending'` as the fence.
- `mark_delivered`, `mark_delivery_failed`, `release_delivery`, `retry_failed`.
- `list_delivering_for_reconcile` — bounded at
  `USER_PROMPT_QUEUE_MAX_RECONCILE` (256).
- `resolve_stranded_delivery` — guarded on `delivering` so a startup sweep
  racing a live delivery cannot undo it.

### The three `status = 'pending'` literals

All three were widened, and are now **one constant**
(`LIVE_STATUS_PREDICATE = "status IN ('pending', 'delivering')"`) because they are
one fact:

| Was | Now | Consequence if left |
|---|---|---|
| `list_live` sweep | covers `delivering` | A claim made under the TTL outlives it forever |
| `list_live` select | covers `delivering` | A delivering entry **vanishes from the client's list** — reads as data loss |
| `check_capacity` / `_in` | counts `delivering` | The backlog bound is exceeded by however many deliveries are in flight |

Splitting them again is how a `delivering` row becomes invisible, uncounted, and
unexpired at once, which is the exact defect M003 exists to close.

### Operable is narrower than projected

`delete`'s renumber displaces every survivor into a high band, and `reorder`
requires the client's list to be a permutation of it. Both need the **mutable**
set, so `list_live_in` stays `pending`-only while `list_projected_in` (new)
returns `pending` + `delivering`. Folding a claimed row into the operable set
would renumber a row whose ordinal is bound to an in-flight admission.

A `delivering` entry therefore returns the **new** typed refusal
`user_prompt_queue_entry_delivering`, not `user_prompt_queue_expired`. Collapsing
the two was accurate while every non-`pending` row was terminal; now it would be
a lie a client may render to a user.

## 5. A real defect found and fixed during this milestone

`claim_next_for_delivery` originally selected the next due **id**, then re-read
`WHERE status = 'delivering' ORDER BY ordinal ASC LIMIT 1` to obtain the record.
With a row stranded at a lower ordinal — the exact state the restart twin exists
to repair — that second read returned the **stranded** entry. The caller would
then submit the stranded entry's text while marking the entry it actually
claimed as `delivered`: one prompt executed, another recorded against it.

Fixed by selecting the candidate **by identity** and transitioning that exact
row. Regression test:
`user_prompt_queue_claim_returns_its_own_entry_not_a_stranded_one`.

**The test was proven load-bearing, not assumed.** The original query was
re-injected and the test run against it:

```
test user_prompt_queue_claim_returns_its_own_entry_not_a_stranded_one ... FAILED
assertion `left == right` failed: a claim must return its own entry, never an older stranded one
  left: "entry-0"
```

The fix was restored and the suite re-run: 16 passed, 0 failed.

## 6. Work package C — the named entry and its guard

`CoreDaemon::submit_queued_prompt_turn` in the new
`src/core/daemon_queue_delivery.rs`. It is the **only** daemon-internal delivery
entry, and it does exactly what an ordinary client submit does — it calls
`handle_turns_request(CoreRequest::TurnSubmit { .. })`. There is no
queue-specific submit, no relaxed receipt, and no replay of an interrupted one.

**At-most-once is inherited from `turn_admission_receipt`.** The submission id is
a deterministic hash of the entry id alone
(`queued_delivery_submission_id`), so a retry after a restart re-derives the same
receipt and receives `Duplicate`.

Pinned by
`user_prompt_queue_delivery_submission_id_is_a_pure_function_of_the_entry_id`,
which asserts same-entry sameness, cross-entry distinctness, namespace
separation from ADR-0031's `idle-` prefix, and that the companion digest treats a
change of entry, session, owner, **or** content as a conflict rather than a
replay. Fold a timestamp, an attempt counter, or a claim nonce into that function
and at-most-once stops holding while every other test still passes.

### Failure discipline

Every refusal resolves the claim; an outcome that left it claimed would be a
stranded row awaiting the next restart, which is a defect rather than a state to
model. A submission that **never reached admission** is *released* back to
`pending`; one that reached admission and was **refused** is terminal `failed`.
The split is on the typed `AppError` class (`Storage`/`Io` transient), **never on
error wording** — which is what stops a transient fault from silently consuming
a user's prompt.

### The guard

`scripts/check_queue_delivery_callsites.py`, in the shape of
`scripts/check_idle_continuation_callsites.py`. Five checks:

1. The entry is defined once, in `daemon_queue_delivery.rs`.
2. Every call outside the definition is inside one of two permitted callers in an
   allowed file.
3. **Both permitted callers are actually wired.** A guard that passes on an
   unwired feature constrains nothing — this check was added after the guard
   initially passed with zero call sites, which was a false pass.
4. `CoreRequest` carries no queue-delivery surface and still has `TurnSubmit`.
5. The module references neither `is_controller_bypass` nor a synthetic
   principal, and does use `reconstructed` + `AuthMethod::QueueDelivery` +
   `CoreRequest::TurnSubmit`.

**Self-test, and proven non-vacuous:**

```
$ python3 scripts/check_queue_delivery_callsites.py --self-test
check_queue_delivery_callsites --self-test
  ok: injected third caller rejected
  ok: unmodified tree still passes
```

Registered in `scripts/verify.sh` (quick) **and** `.github/workflows/ci.yml`.

The ADR-0031 guard was re-run as a regression and still passes; widening
`submit_idle_continuation_turn` was rejected and did not happen.

## 7. Work package D — two triggers, and no poller

| Caller | Where | When |
|---|---|---|
| `deliver_next_queued_prompt` | `src/core/daemon_control.rs`, from `spawn_turn_reaper` | after a terminal event, the lease is released, and the runtime is parked `Idle` |
| `reconcile_stranded_queue_deliveries` | `src/core/daemon_bootstrap.rs`, `recover_state` | once per start, after `interrupt_incomplete_turn_receipts` **and** after `reconcile_session_controllers` |

The reaper clones only `sessions` and `pool`; it re-enters the daemon through
the **already-published** `idle_wake_cell` `Weak` rather than adding a second
publication surface.

Restart-twin ordering is load-bearing and was reasoned, not guessed: before
`reconcile_session_controllers`, a session holding a stale lease would release
every entry it tried to deliver and defer it to a trigger that never comes;
before `interrupt_incomplete_turn_receipts`, a receipt the daemon killed
mid-turn would be read before it had been told the truth about itself.

**Neither trigger is a poller.** The daemon still has no periodic
session-observation loop: its three `tokio::time` intervals remain projection
retention (300s), opt-in provider refresh (30s), and opt-in TTS drain (500ms).
Delivery runs on a terminal transition and at restart — the only two moments a
session is free by construction.

Stranded rows are resolved **from the receipt**, never from the entry, because
the receipt is the only evidence of whether a turn started.

## 8. Work package E — projection push, and one security decision

Delivery is daemon-initiated with no reply channel, so no client-initiated read
can discover it. Before this milestone the queue was read on exactly one edge —
the session change — and a delivery produces none.

- **`CoreEvent::UserPromptQueue` added** (additive, `#[serde(default)]`), distinct
  from the same-named `CoreResponse` variant: that one answers a client's
  `UserPromptQueueList`; this one has no request behind it.
- `CoreDaemon::publish_queue_changed` re-reads and pushes after any delivery
  attempt that changed the queue.
- `App::apply_core_event` adopts it through the **same**
  `apply_user_prompt_queue_loaded` applier the read path uses, so a pushed and a
  read projection cannot diverge in filtering — the superseded-session guard
  included.
- `user_prompt_queue_entry_to_dto` now carries the **real** status. It
  hardcoded `"pending"` on the reasoning that every `list_live` row was pending
  by construction — true until `delivering` gained a producer.

### `CoreEvent::UserPromptQueue` is `ClientLocal`, not `Safe`

The projection carries **user prompt text**. `Safe` would be a real widening:
the `UserPromptQueueList` request path is gated by a descriptor requiring
`session.read`, but `event_log.publish` feeds the projection sink as well as
the frontend stream, so `Safe` would place a user's queued prompt into the
durable projection store behind a check the request path never ran. `ClientLocal`
reaches the session's own frontend stream — where the push must land — and is
denied at the projection disclosure boundary.

Audit: `user_prompt_queue_delivery_event` emits `prompt_submit` with the
delivering principal as actor, entry id, turn id, and a **byte count** — never
text, never a digest. `("user_prompt_queue_deliver", "prompt_submit")` added to
`INSTRUMENTED_OPERATIONS`.

## 9. Verification actually run

| Check | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo check --lib` | clean, no warnings |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `cargo test -p codegg-core --test user_prompt_queue` | 22 passed |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | 16 passed |
| `cargo test -p codegg-core --test user_prompt_queue_mutation` | 25 passed (M002b, unregressed) |
| `cargo test --test identity_m003_daemon_authorization` | 23 passed (19 M002b + 4 M003) |
| `scripts/check_queue_delivery_callsites.py` | 5/5 pass |
| `scripts/check_queue_delivery_callsites.py --self-test` | pass, non-vacuous |
| `scripts/check_idle_continuation_callsites.py` | pass (ADR-0031 unregressed) |
| `scripts/check_audit_coverage.py` | pass |
| `scripts/check-core-boundary.sh` | pass |
| `scripts/check_tui_transport_authority.py` | pass |
| `scripts/check_scheduler_bypass.py` | pass |
| `scripts/check_http_route_disposition.py` | pass |

**Not run:** `verify.sh quick` in full, `verify.sh full`, and the workspace
suite. No claim is made about them. Hosted CI was not run and is not claimed —
the CI change (the new guard step) is unverified by a CI run.

**Test filter honesty:** the queue suites use a `user_prompt_queue` name prefix
and were run per-target, not by a zero-select global filter. 63 queue tests plus
23 identity tests ran and passed. Nothing was skipped for being unselectable.

## 10. What this milestone did not do

- **No UI.** M004 (`/queue` box) and M005 (editing) own everything a user can
  press. M004's blocker is now discharged except for the shared
  `tui-panel-surface-convergence` chrome/geometry helper.
- **No delayed delivery.** `run_at_ms` and M007 are untouched, and v82 remains
  M007's.
- **No steering.** ADR-0036 remains `proposed` and still gates M006.
- **No desktop parity.** M008.
- **No new migration**, and no edit to v80 or v81.

## 11. Durable facts a later milestone must not undo

1. **The delivered principal is the owner.** A synthetic daemon identity would
   take the controller lease from the owner and lock it out of its own queue at
   its first mid-turn mutation. This is the single most important property here.
2. **Three literals, one constant.** Visibility, backlog counting, and expiry
   for `delivering` must not be re-derived separately.
3. **Released is not failed.** Never-reached-admission releases; reached-and-
   refused is terminal.
4. **At-most-once comes from the receipt.** Any change to
   `queued_delivery_submission_id` silently destroys it, because the whole
   at-most-once argument is that the id is a pure function of the entry id.
5. **Two triggers only.** A third caller is a recorded decision, and the guard
   must say so before it exists.
6. **`CoreEvent::UserPromptQueue` is `ClientLocal`.** Promoting it to `Safe`
   puts prompt text into the durable projection store.
7. **Failure reasons are ≤64-byte codes.** Provider error text can quote the
   prompt, and the column outlives the failure.

## 12. Residual risks

| Risk | State |
|---|---|
| Owner deleted between claim and authorization | Fails closed, entry released, delivered on a later trigger after ownership is repaired |
| Receipt admitted but the entry binding fails to land | Deliberately left `delivering`; the startup sweep resolves it from the receipt. Releasing instead would risk a duplicate delivery |
| Model/provider unresolvable at delivery | Released, not failed — retried on the next trigger. A permanently unresolvable selection would retry indefinitely |
| Stale `delivering` rows beyond the 256 bound | Left `delivering`; picked up next startup or the next post-terminal wake for that session |
| The delivery-authority tests assert refusal paths | The test daemon has no provider selection, so composition fails closed and no receipt is written. A **positive** receipt-identity assertion needs a fixture with a resolvable model, which no existing test harness provides; it is recorded as an open verification gap rather than claimed as covered |

## 13. Open items for the maintainer

1. **The positive delivery-authority test.** §12's last row. The headline claim —
   that the receipt records the owner — is currently asserted structurally
   (descriptor, classification round-trip, owner-row provenance) rather than by
   observing a real receipt. It should be closed when a test harness with a
   resolvable model exists.
2. **`verify.sh full` and the workspace suite** were not run for this milestone.
3. **The whole tree remains uncommitted** — M001, M002a, M002b, M003, and every
   planning artifact.