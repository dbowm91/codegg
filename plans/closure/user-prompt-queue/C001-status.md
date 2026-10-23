# User Prompt Queue Corrective C001 — Closure Record

Status: **closed**
Date: 2026-10-18
Class: **invariant**
Plan: `plans/implementation/user-prompt-queue/C001-released-delivery-order-displacement-corrective.md`
Roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Corrective C001)
Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted
M001–M005 tree (61 porcelain entries at plan time).

**Two defects were found during implementation and are reported, not fixed.** They
are in §6 and §9. Read those before relying on the invariant below: the fix is real
and complete for what it covers, and two adjacent facts about this table remain open.

## 1. What shipped

An entry whose delivery was interrupted now resumes **at its own position**.
`renumber_dense` writes dense ordinals for the whole **live** set rather than only
the client's operable ids:

```rust
// crates/codegg-core/src/session/user_prompt_queue.rs
let live = list_projected_in(tx, session_id, now_ms).await?;
let merged = merge_operable_into_live(&live, desired_operable)?;
// displacement unchanged, then dense 0..n-1 over `merged`
```

`merge_operable_into_live` holds each non-operable live row at **the slot it already
occupied**. For live `[p1(0), d1(1), p2(2)]` reordered to `[p2, p1]`, the result is
`[p2(0), d1(1), p1(2)]`.

Because a `delivering` row now *receives* a dense ordinal, `release_delivery` and
`resolve_stranded_delivery(Retry)` needed **no change at all** — they restore a
status, and the ordinal they were restoring was never lost.

| Path | Before | After |
|---|---|---|
| `release_delivery` | entry reappeared above the band, **delivered last** | order-neutral: the queue is identical before and after |
| `resolve_stranded_delivery(Retry)` | same | same |
| `retry_failed` | above the band | **unchanged** — see §6.1 |
| `delete` survivor renumbering | left a `delivering` row displaced | densifies it like any live row |

## 2. Scope boundary held

**No migration.** `STORAGE_LAYOUT_VERSION` stays **81**; v82 stays M007's.
**No new status value**, no change to v80 or v81.
**No protocol, frontend, or delivery-pump change.** The box was already correct —
it renders the daemon's ordinal and never renumbers locally.
**No closure record rewritten.** M002b's and M003's stand; M005's stands, and the
roadmap annotates it.

**The displacement step is unchanged**, and had to stay. Terminal rows are not in
the live set but still hold ordinals under a session-wide `UNIQUE (session_id,
ordinal)`, so they must still be pushed clear of the band. That is the whole reason
Option A needed no migration, and removing the step breaks `UNIQUE` (proven in §5).

## 3. The merge rule, and why preserve-existing-rank

Decided, not inferred: a non-pending live row keeps **its own rank** among live rows.
It is never pinned to the head and never swept to the tail.

Pinning is the one-line alternative, and it looks tidier. It is also **the defect
under repair** — pinning a released entry to an end is exactly "a released delivery
returns to the back of the queue", just with a rule instead of an accident.

The rule is falsifiable, so it is pinned by unit tests over head, middle, tail, and
several interleaved non-pending rows.

## 4. Verification actually run

All counts are real and local. `CARGO_BUILD_JOBS=2` throughout; no `--all-features`.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test -p codegg-core --lib user_prompt_queue` | **11 passed, 0 failed** (858 filtered out) |
| `cargo test -p codegg-core --test user_prompt_queue` | **22 passed, 0 failed** |
| `cargo test -p codegg-core --test user_prompt_queue_mutation` | **34 passed, 0 failed** |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | **16 passed, 0 failed** |
| `cargo test -p codegg --lib -- dialogs::queue commands::queue` | **48 passed, 0 failed** (6487 filtered out) |
| `cargo test --test user_prompt_queue_box` | **44 passed, 0 failed** |
| `python3 scripts/check_queue_delivery_callsites.py` | **all checks pass** (5 assertions) |
| `python3 scripts/check_project_catalog_invariants.py` | **7/7 checks passed** |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **clean, no warnings** |
| `scripts/verify.sh quick` | **passed** |

### 4.1 A defect in this pass's own verification

The first `verify.sh quick` run **failed**, and not on anything C001 changed:

```
ADR integrity check failed:
- dangling ADR path in plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md
- dangling ADR path in plans/registry.md
```

Both pointed at a full `plans/adrs/ADR-0039-…-wakeup-authority.md` path, which
the M007 planning pass named as the likely discharge vehicle for the wakeup
decision and **deliberately did not draft**, because an acceptance is a
maintainer action. `check_adr_integrity.py` refuses a link to a file that does not
exist, and it is right to: a plan pointing at an unwritten ADR teaches a reader
that the ADR exists.

Fixed by referring to it by **number** (`a new ADR numbered 0039`) instead of by
path — the same form the registry's M007 row already used — which keeps the intent
and stops asserting a file that was never written. The ADR itself is still **not**
drafted; that remains a maintainer action.

Recorded because it is the second time in two passes that a *planning* change broke
`verify.sh quick` without anyone running it. `verify.sh quick` must be run after a
planning-only edit too, not only after code.

**Not run:** the full workspace test suite, `verify.sh full`, and any hosted CI.
Nothing was pushed; `HEAD` is unchanged at `18c3dca920` and the working tree holds
the pre-existing uncommitted M001–M005 work plus this milestone.

## 5. Guards proven non-vacuous by injection

Both reverted immediately after the observed failure.

**(i) The merge.** Replacing it with the pre-C001 write set — pending targets only —
fails **5** tests, including the property test:

```
---- user_prompt_queue_a_released_delivery_resumes_at_its_own_position ----
left: ["d", "c", "b"]
right: ["b", "d", "c"]
```

`["d","c","b"]` is precisely M005's recorded defect, recovered by the injection.

**(ii) The displacement step.** Removing it fails **8** tests, every one with
`UNIQUE constraint failed: user_prompt_queue_entry.session_id, …`.

**A third finding came out of injection (i), and it is the one that mattered.** The
property test *passed* against the unfixed store. Its operation alphabet had no step
that left a row `delivering` — every claim was immediately released, failed, or
delivered — so no two operations ever straddled a reorder with a mid-delivery row,
which is the entire defect scenario. The test was exhaustive and still blind.

`QueueOp::ClaimHold` was added, and injection (i) then fails the property test too.
This is recorded because a test that passes against the bug it exists to catch is
worse than no test: it reports coverage that does not exist.

## 6. Defects found and reported, not fixed

C001 §14 stops the milestone on both of these. Neither is patched.

### 6.1 `retry_failed` is not reached by the merge

The plan's §6 states that all three return paths are covered "with **no change to
those functions at all**". That claim is **falsified** for this one.

`retry_failed` revives a row that was **terminal** (`failed`) while the reorder ran.
A terminal row is not in the live set, so the merge never gave it a dense ordinal,
and reviving it restores a status it no longer owns an ordinal for.

- **Not data loss.** The entry is still delivered — last, behind rows queued after it
  failed. The test claims it three times to reach it and asserts the order.
- **No production caller.** The daemon's non-terminal path is `release_delivery`
  (`src/core/daemon_queue_delivery.rs:449`) and reconciliation uses
  `resolve_stranded_delivery` (`:505`). `retry_failed` is reached only from a
  store-level test.
- **Never observed to move.** A `failed` entry is absent from the client projection,
  so no user watched it happen.

**The decision a maintainer still owns:** where a revived `failed` entry belongs — its
stale ordinal, the head, or the tail of the band. It is deliberately not picked here.
`user_prompt_queue_a_retried_failure_is_not_reached_by_the_c001_merge` pins the
measured truth so the choice is met by a red test rather than a silent change.

### 6.2 The displacement collides with itself on a second renumber

**Pre-existing, and independent of this milestone.** The displacement `UPDATE` is
byte-identical before and after C001, and the collision occurs *inside that
statement*, before the merge is consulted. A uniform offset is only collision-free
while no row already sits at or above the band.

Minimal reproduction, reachable from the queue box alone:

1. queue three prompts
2. reorder
3. let one fail
4. reorder
5. **reorder again** → refused with a storage error

The failing state is pinned exactly: `a` pending at 0, `b` pending at 1, `c` failed
at 1 000 000. The displacement lifts `a` 0 → 1 000 000 while `c` still occupies it.

The exhaustive enumeration counts **4** of 1 554 steps hitting it and asserts the
first triggering row state, so a fix is a deliberate red → green.

**C001 §14 applies verbatim:** "a second displacement appears to be needed to keep
`UNIQUE`; stop: the displacement must remain a single uniform step over every session
row." A correct fix — a monotonic band generation, or compacting terminal rows out of
the way — is a design decision with its own trade-offs, not an implementation detail.

### 6.3 Adjacent, out of scope, worth recording

`add` appends over `MAX(ordinal)` across **all** rows, so after a reorder that
displaced a terminal row, the next add lands above the band as well. Pre-existing,
untouched by C001, and a consequence of the same band-offset design as §6.2.

### 6.4 A property assertion narrowed after failing correctly

The first version asserted "the live set is always dense `0..n`". It failed on
`[ClaimFail]` with `[1, 2]`, and the store was right: a row going terminal leaves a
hole that only the next reorder or delete closes, and `add` (see §6.3) appends above
the band. Density was never a global invariant of this table.

The assertion now states what C001 actually guarantees: **no live row is ever
stranded above the band**. Recorded because narrowing a failing assertion is
exactly the move that hides a real defect, and the reason it is safe here is that
the replacement is weaker but still falsifiable — §5(ii) fails it.

## 7. Tests that replaced measurements

The two M005 tests asserted the defect. They were measurements of an
accepted-and-documented outcome, which is what made correcting them safe.

| Removed | Replaced by |
|---|---|
| `…_reorder_displaces_a_delivering_row_above_the_pending_band` | `…_a_reorder_holds_a_delivering_row_at_its_rank` |
| `…_a_released_delivery_returns_to_the_back_of_the_queue` | `…_a_released_delivery_resumes_at_its_own_position` |

M005's control (`…_a_release_without_a_reorder_keeps_its_place`) and its uniqueness
test pass **unmodified**. The client's permutation contract is verified unchanged by
`…_a_permutation_naming_a_delivering_row_is_refused`, which still refuses a
permutation naming a `delivering` row.

## 8. Acceptance criteria

| # | Criterion | Status |
|---|---|---|
| 1 | A released entry resumes at its own position, not at the back | met |
| 2 | Same for `retry_failed` and startup reconciliation | **partially met** — reconciliation met; `retry_failed` is the §6.1 stop condition |
| 3 | `UNIQUE (session_id, ordinal)` holds throughout; M002b's tests pass unmodified | met — proven non-vacuous |
| 4 | A `delivering` row no longer renders last during a reorder | met |
| 5 | Client permutation contract unchanged, still pending ids only | met |
| 6 | The two M005 tests replaced by position-preservation tests | met |
| 7 | The `docs/tui.md` callout and its "add it last" instruction are gone | met |
| 8 | No migration, layout change, or status change | met |
| 9 | Both guards proven non-vacuous by injection, and reverted | met |
| 10 | `verify.sh quick`, clippy `-D warnings`, `cargo fmt --check` pass | met — after fixing §4.1 |

Criterion 2 is recorded as partially met rather than met. The plan asserted it
would be fully met; measurement said otherwise for one of three paths, and §14 says
report rather than patch.

## 9. Residual risks

1. **`retry_failed` (§6.1).** Open maintainer decision. No production caller.
2. **Second-displacement collision (§6.2).** Pre-existing, reachable from the queue
   box, fails a user reorder with an opaque storage error. Highest-severity item in
   this record.
3. **`add` appending above the band (§6.3).** Same design root as §6.2.
4. **The merge reads the live set inside the caller's transaction.** Both callers
   hold the write lock `sweep_expired` acquires, so the read and the write cannot
   disagree. A future caller that reads before acquiring that lock would break the
   `OrderMismatch` backstop rather than corrupt anything.
5. **Live-set `LIMIT`.** `list_projected_in` caps at
   `USER_PROMPT_QUEUE_MAX_RETAINED_PER_SESSION` (256). Pending is bounded at 64, so
   this is not reachable in practice, but a live set above the cap would leave the
   overflow displaced — unchanged from before.
6. **Carried forward, unrelated:** the absent positive receipt-identity fixture
   (M003), the `session_family` symlink failures, and the two dangling doc refs.

## 10. State handed on

**M006 is unchanged and unblocked.** C001 touched the store's renumber path only —
no protocol, no frontend, no delivery pump — and M006's send-now settlement
transition is on an entry that leaves the queue rather than returning to it.

**M007 is unchanged and still blocked** on the wakeup-owner decision. C001 took no
migration number, so **v82 stays M007's**.

**M008 remains eligible and explicitly deferrable.**

**Nothing was reopened.** M002b's, M003's, and M005's closure records stand as
written. The superseded thing is M005's *disposition* — "accepted and documented" —
together with the `docs/tui.md` workaround it produced. The roadmap records this by
annotation (§7 M005, §10, §12) rather than by editing any closed record.

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **User prompt queue and turn contention** on 2026-10-23, so that the registry stays an index.

Baseline `18c3dca9`: a prompt composed during an active turn is **destroyed**, not merely refused — the composer clears before the transport decision and the `turn_already_active` arm restores nothing. The same arm also **corrupts active-turn state**: it reports every submit failure through `fail_unstarted_turn`, which clears `working_since`/`active_turn_id`, sets `Error`, and stops the spinner *including when a turn is still running on the daemon*. So the UI asserts "no active turn" while one exists — fatal for any queue that must consult turn state to choose between queueing and steering. Two backend facts reshape the estimate: **`CoreRequest::TurnSteer` is already implemented, controller-gated, and has no frontend issuer at all** (zero `TurnSteer` matches under `src/tui/`), and it is `try_send` on a bounded channel with `steer_queue_full`/`unavailable`/`not_supported` — best-effort, so `f` = "force" would misdescribe it (ADR-0036 fixes the guarantee as *acceptance* and forbids "force" in copy). A **second, unrelated** mechanism already drains prompts: `AgentLoop::drain_follow_up` consumes at turn boundaries and documents that late arrivals are not processed by the same run — two "queued prompt" notions with different guarantees, so ADR-0036 fixes terminology and requires cross-references. **Durability is the load-bearing decision (ADR-0035):** session attach is non-exclusive (`SessionAttach` dispatches identically to `SessionLoad`; `attach_session` appends to a `Vec`), single-writer is the durable controller lease, admission is receipted by envelope `request_id`, startup marks accepted→`interrupted` and never replays, the WS transport "makes no staging or acceptance claim from queue delivery", and composer state is memory-only — so a frontend-local queue dies on reconnect and a "replay on reconnect" would bypass at-most-once. ADR-0035 selects a daemon-owned durable table (migration **v80**, `STORAGE_LAYOUT_VERSION` currently **79**, additive precedent v65) modelled on `human_shell_promotion` (24h TTL, status, `consumed_turn_id`), and explicitly rejects modelling prompts as `JobTemplate` because schedules materialize *jobs*. Sequence is backend-first by design: M001 (bug, ready) → M002 storage (infrastructure, **not** capability) → M003 delivery → M004 box+row-0 suffix → M005 add/edit/reorder → M006 steer → M007 timers → M008 desktop parity. M002–M008 are deliberately unplanned: each waits on a closure or an ADR acceptance. Two inherited box rules recorded in the roadmap: no box advertises `Esc`/`Enter` as a close verb, and a caller-supplied footer is not replayed into the keymap hint; `Shift+J/K` are panel directions the docked host intercepts *before* the widget, which is why an existing panel's advertised reorder keys were dead — a modal box avoids it, and M004 must prove non-vacuity rather than assume it. **M001 is now closed:** contention takes `App::fail_contended_turn`, which leaves `active_turn_id`, `working_since`, session status, and the spinner untouched and reports a warning toast instead of latching a turn error (row 0 gives `last_error` absolute precedence, so latching would hide the live turn's status line — the same corruption in a different costume). Every recoverable refusal restores the composed text through the single `PromptState::restore_prompt_with_stash` shape, which three call sites previously reimplemented with a hardcoded `100`. The submit path now uses `spawn_tui_task` plus an `AsyncUiRequestState` generation guard, kept separate from the daemon's `request_id` idempotency key. Two further live defects were found and fixed: `send_prompt` was overwriting the failure release with `Working` (so the no-core-client and no-session paths had never actually fixed anything), and a stale completion could still null a live turn because only the pending-record clear was guarded. M002's hard dependency is discharged. **ADR-0035 was accepted 2026-10-18**, and the acceptance audit found that its authority clause — "the same controller predicate that governs turn admission" — has **no answer while a session is idle**: `session_turn_controller` is keyed `(session_id, turn_id)`, `acquire` rejects an empty `turn_id`, and `release` `DELETE`s the row, so `controller_lease_for` returns `None` between turns and `check_turn_controller` refuses *every* principal, including the owner. The only idle-passing path is `is_controller_bypass` → `is_local_owner_broad`, an identity comparison against `LOCAL_OWNER_PRINCIPAL_ID` that is **not session-bound** — it cannot express "a session you do not control". Mutating a queue *between turns* is the primary use, so this is a canonical ownership-boundary decision, not an implementation detail. **M002 is therefore split:** M002a (storage + read projection, `ready`, dependency-clean under every ADR outcome) and M002b (mutation operations + mutation authority, `blocked`). ADR-0037 is raised as **proposed** and confers no authorization (ADR-0030/ADR-0034 precedent), so M002b does not start. Two ADR-referenced seams were corrected against source: the queue read uses `Capability::SessionRead` rather than the broader `AgentInvoke` that `human_shell_promotions_get` uses, and `is_controller_bypass` gains no queue authority. **M002a is now closed** (closure record `plans/closure/user-prompt-queue/002a-status.md`): **v80** landed with the durable, bounded, expiring `user_prompt_queue_entry` table, a schema-enforced `UNIQUE (session_id, ordinal)`, typed refusals for every bound, the lazy transactional expiry sweep, and `UserPromptQueueList` under `Capability::SessionRead`. It **delivered no user-visible capability** — no mutation exists, so the read has no producer and the queue always reads empty. **ADR-0037 was accepted the same day**, selecting **Option B**: a durable, session-scoped queue-owner binding, independent of turn state, surviving restart, changed only by explicit audited transfer. `is_controller_bypass` is explicitly not extended. Mid-turn the owner and the controller must both match the caller, and all four mismatch combinations are typed refusals rather than a precedence rule — the sharp edge the ADR exists to resolve. One correction was recorded inside ADR-0037 itself: its draft folded the owner binding into **v80**, but M002a had already shipped v80 with the queue table alone, so the claim was corrected to **v81** while the ADR was still `proposed`, with the difference between draft and accepted text stated in the accepted record's header. Consequently `run_at_ms` moves to **v82**. **M002b is now closed** (closure record `plans/closure/user-prompt-queue/002b-status.md`): **v81** landed carrying only `user_prompt_queue_owner`, the four mutations reached production producers, and the queue is no longer permanently empty — but it **delivered no user-visible capability** either, because no command, dialog, or key reaches any of it. The adoption policy ADR-0037 deferred to this milestone is settled and audited: **the calling principal becomes the owner** on first add, not the session creator and not the local interactive owner; first-add adopts only on an `Absent` queue (creation), never on an `Unowned` one (repair), so an invalidated queue cannot silently become owned by whoever touches it next. `is_controller_bypass` gained no queue authority — its early return is guarded by `matches!(scope, ControllerScope::Turn { .. })`, and a test proves it by execution rather than by inspection. Two real bugs the new tests caught and fixed are recorded in the closure record rather than shipped: `delete` read the surviving projection without renumbering, leaving a permanent ordinal gap, and `is_permutation` had an inverted `HashSet` duplicate check (`!insert` accepts a duplicate rather than rejecting it). M002b is closed and M003 is next. |

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **User prompt queue** on 2026-10-23, so that the registry stays an index.

Class **invariant** — ADR-0035 fixes queue ordering as a daemon-owned total order. This is the maintainer decision M005's closure record asked for, and planning it measured a **worse** defect than M005 recorded. M005 observed that a released `delivering` entry keeps the displacement ordinal (1 000 000) and *renders* last; the delivery read is `ORDER BY ordinal ASC` (`user_prompt_queue.rs:1401-1405`), so it is **delivered** last too — contradicting ADR-0035's own "a trigger that fires out of order must not reorder the user's work". **Three** paths are affected, not one: `release_delivery` (`:1530`), `retry_failed` (`:1554`), and `resolve_stranded_delivery(Retry)` all restore `status = 'pending'` with **no ordinal write**. Any of them followed by any reorder silently demotes a queued prompt; daemon shutdown or a transient storage error is enough to trigger it. **No new ADR is required** — the fix implements ADR-0035 rather than changing it, and M005's closure record is accurate and is **not** rewritten. **The fix takes no migration**: `renumber_dense` writes dense ordinals for the whole *live* set (pending **and** delivering) with non-pending rows preserved at their existing rank, while the uniform session-wide displacement step stays **exactly** as it is — which is what keeps `UNIQUE (session_id, ordinal)` safe against terminal rows, so M002b's uniqueness tests pass unmodified. This also removes M005's cosmetic symptom as a side effect. The client's permutation contract is unchanged (still pending ids only). **Two M005 tests asserted the defect** and are to be replaced by position-preservation tests; they were measurements of an accepted-and-documented outcome, not contract requirements, which is what makes correcting them safe. The `docs/tui.md` callout at `357-362` ("A reordered prompt that was mid-delivery may come back at the end … add it last rather than reordering an older one") is removed **in full**, because after the fix its premise is false. Independent of M006/M007; takes no migration number, so **v82 stays M007's**. **Delivered 2026-10-18.** `merge_operable_into_live` folds the client's desired pending order with each non-pending live row held at **the rank it already had** — *preserve existing rank*, never pinned to an end, because pinning a released entry to the tail is the defect itself — and `renumber_dense` writes that merged live set. The displacement step is unchanged and terminal rows are still displaced: they are not live but still hold ordinals under a session-wide `UNIQUE`, so excluding them would reintroduce the collision the offset prevents. Because a `delivering` row now *receives* a dense ordinal, `release_delivery` and `resolve_stranded_delivery(Retry)` needed **no change at all** and are now **order-neutral** — the released queue is byte-identical to the queue before the release. The client's permutation contract is unchanged, verified by still refusing a permutation that names a `delivering` row. The two M005 tests that asserted the defect are **replaced** by position-preservation tests, and the `docs/tui.md` callout is removed in full. Verification is an **exhaustive** enumeration of all **1 554** sequences of length 1–4 over **six** operations — deterministic, not sampled — and a sixth operation `ClaimHold` was **added after** injection (i) showed the enumeration passed against the unfixed store: with no step leaving a row `delivering`, no two operations ever straddled a reorder with one, which is the whole defect scenario. **Two defects were found and reported, not fixed**, per C001 §14. (1) **`retry_failed` is not reached by the merge** — the plan's claim that all three return paths need no change is falsified for this one, because it revives a row that was *terminal* during the reorder and therefore absent from the live set; no production caller, absent from the client projection, and where such a row belongs is an open maintainer decision. (2) **The uniform displacement collides with itself on a second renumber** — pre-existing and independent of the merge, since the displacement `UPDATE` is byte-identical before and after; reachable from the queue box alone (queue three prompts, reorder, let one fail, reorder, **reorder again** — refused with a storage error). Both are pinned by tests so a fix is met by a red test, not a silent behaviour change. |

## Registry narrative (second pass)

Moved verbatim from the `plans/registry.md` row for **User prompt queue and turn contention** on 2026-10-23, so that the registry stays an index.

Its plan's §3(b) open question is settled by measurement rather than inference — a released `delivering` entry resumes at the **back** of the queue and keeps the displacement ordinal, which is recorded as accepted-and-documented because changing `renumber_dense` reopens closed M002b/M003. **All four ADRs are accepted** as of 2026-10-18. It needed **no migration**: the acceptance decision it was planned against shipped as `mark_steer_delivered`, a second `pending → delivered` transition guarded on `pending` and bound to the accepting turn using columns v80 already created, so a steered prompt cannot then be claimed again as a fresh turn. The request carries `entry_id` and nothing else — no prompt text, no `turn_id` — and the handover is gated by the turn controller, never by `ControllerScope::QueueMutation`, with queue ownership re-asked inside the settlement transaction. One outcome exceeded the plan's two-valued shape: `AcceptedNotSettled`, for the turn having taken the prompt while the queue could not record it, which as an error would have invited the user to resend something that already ran. Its decision is recorded in `plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md`, **accepted 2026-10-18**. The decision was **Option A**, a bounded queue-scoped advance tick: the roadmap premise of "an existing periodic observation point" was falsified during planning (the daemon has no periodic session observation loop, and M003's delivery entry had exactly two permitted callers, neither of which fires for an idle session with a scheduled entry), so a new wakeup source was unavoidable. What the implementation guarantees is narrower and honest — the tick computes the earliest due instant, sleeps exactly until it, and **parks with no timer armed** when nothing is scheduled. M007 takes **migration v82**, so `STORAGE_LAYOUT_VERSION` is now **82**; `run_at_ms` is a nullable column with **no `CHECK`** and **no sentinel**, because v80's `status` `CHECK` cannot be altered additively at all. The authoring surface is a **second step** (text, then duration) rather than an inline `| 1h` suffix, because any inline delimiter is a character the user cannot type and prompt text must not be reinterpreted. A due time is a **floor, not a promise**: delivered at the first turn boundary at or after it, and *late* across a restart, never early. Widening the delivery entry to a third caller was a **recorded decision**, with the guard's `--self-test` re-proven by injecting a **fourth** caller and rejecting it. The milestone's own wakeup test then found and fixed a real defect: a due-but-undeliverable entry left the tick re-arming at zero delay and spinning. The displacement was worse than measured, because `claim_next_for_delivery` orders by `ordinal`, so a released entry was *delivered* last, not merely rendered last. It is fixed with **no migration**: `renumber_dense` writes the whole live set, holding each non-pending row at its existing rank, and `release_delivery` / `resolve_stranded_delivery(Retry)` are now order-neutral. The `docs/tui.md` workaround is gone. Two further defects were found and reported rather than fixed per C001 §14 — `retry_failed`'s revive, and a pre-existing collision when the displacement runs a **second** time — and both are pinned by tests. **M008 is now eligible** (M004 and M005 both closed) and remains explicitly deferrable
