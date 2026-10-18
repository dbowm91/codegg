# User Prompt Queue Corrective C001 — Released Deliveries Return at the Back of the Queue

Status: **ready for handoff**

Repository baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the
uncommitted M001–M005 tree (61 porcelain entries, 17 untracked, at plan time).
Re-inspect before implementing; §3 records measured facts at this baseline only.

Source roadmap:
`plans/subsystems/user-prompt-queue-roadmap.md#10-risks-and-decision-points` (the
"released `delivering` entry jumps to the end" row) and
`plans/subsystems/user-prompt-queue-roadmap.md#7-milestones`.

Origin: **a defect against accepted ADR-0035, found and measured during M005.** This is
a corrective pass, not an amendment — it does not rewrite M002b's or M003's records.

Related plans and closures it reopens or corrects:

- `plans/implementation/user-prompt-queue/002b-queue-mutation-operations-and-authority.md`
  / `plans/closure/user-prompt-queue/002b-status.md` — established `renumber_dense`.
- `plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md`
  / `plans/closure/user-prompt-queue/003-status.md` — established the delivery read.
- `plans/implementation/user-prompt-queue/005-queue-box-add-edit-and-reorder.md` /
  `plans/closure/user-prompt-queue/005-status.md` — **§3 and §9.1** measured and
  recorded this behaviour, correctly, as accepted-and-documented.

Applicable ADRs:

- `plans/adrs/ADR-0035-durable-user-prompt-queue-ownership.md` (accepted) — **the
  defect contradicts this accepted contract**; §3.2 quotes it. No new ADR is required
  to fix it, because the fix *implements* the ADR rather than changing it.
- `plans/adrs/ADR-0038-queue-delivery-execution-authority.md` (accepted) — unaffected;
  *as whom* an entry is delivered does not change, only *when*.

Primary class: **invariant**. ADR-0035 fixes queue ordering as a daemon-owned total
order that survives restart; this milestone restores that invariant and earns a property
test rather than only a regression test.

## 1. Objective

Make an entry whose delivery was interrupted resume **at its own position**, so that a
transient storage error, a daemon shutdown mid-claim, or a concurrent reorder can no
longer silently move a user's prompt to the end of their own queue.

## 2. Why this milestone is ready

- The defect is **measured, reproduced, and isolated**, with a control that names the
  cause: M005's `..._returns_to_the_back_of_the_queue` plus
  `..._a_release_without_a_reorder_keeps_its_place`.
- The root cause is identified structurally (§3.1), not inferred from the symptom.
- **No decision is outstanding.** M005's closure recorded that a `renumber_dense` change
  "needs a maintainer decision, not a TUI workaround". This plan is that decision,
  recorded: the fix is Option A below, it needs **no migration**, and it is scoped to one
  store function plus the tests that encoded the defect.
- It is independent of M006 and M007, and of their v82 migration, though the two
  migrations would swap numbers if C001 were ever to need one — under this option it
  does not (§9).

## 3. Current implementation evidence

### 3.1 Root cause: one column serving two incompatible jobs

`renumber_dense`
(`crates/codegg-core/src/session/user_prompt_queue.rs:1242-1274`) does two things:

1. Displaces **every row in the session, status-blind**:
   `UPDATE … SET ordinal = ordinal + 1_000_000 WHERE session_id = ?`.
2. Writes dense `0..n-1` ordinals for the **target ids only** — and the target set is
   the **pending** rows (`list_live_in` → `OPERABLE_STATUS_PREDICATE`,
   `status = 'pending'`), while the client projection carries pending **and** delivering
   (`list_projected_in`, `LIVE_STATUS_PREDICATE`).

So a `delivering` row is displaced and never re-densified. The band offset is what keeps
`UNIQUE (session_id, ordinal)` intact during the write, and it is not itself the bug.

The bug is that **the status transitions back to `pending` never write the ordinal**:

| Path | Effect |
|---|---|
| `release_delivery` (`:1530-1547`) | `status = 'pending'`, no ordinal write |
| `retry_failed` (`:1554-1571`) | `status = 'pending'`, no ordinal write |
| `resolve_stranded_delivery` with `Retry` (`:1625+`) | `status = 'pending'`, no ordinal write |

**All three** return an entry to the queue outside the dense band. M005 measured the
first; the second and third are the same defect and are named here for the first time.

### 3.2 The severity is delivery order, not display — M005 recorded it too gently

`claim_next_for_delivery` selects the candidate with
`ORDER BY ordinal ASC LIMIT 1` over `status = 'pending'`
(`user_prompt_queue.rs:1401-1405`). A returned entry sitting at ordinal ≈ 1 000 000 is
therefore **not merely rendered last — it is delivered last.**

The same function's own doc comment states the intent this violates:

> Ordering by ordinal rather than by age is what makes a queue a queue. A trigger that
> fires out of order must not reorder the user's work.

and ADR-0035 states it as a contract:

> Queue ordering is a total order owned by the daemon, so two frontends cannot interleave
> reorders into an inconsistent sequence.

So this is a defect **against accepted architecture**, which is why no new ADR is needed
and why the closure records must not be rewritten: they are accurate about what was
measured, and the disposition they recorded ("accepted and documented") is what this
corrective pass changes.

The practical trigger is not exotic. Daemon shutdown, a transient storage failure, or a
session switch during delivery all call `release_delivery`. Any of them, followed by any
reorder, silently demotes a queued prompt to the back.

### 3.3 M005's user-facing callout becomes wrong documentation

`docs/tui.md` carries a blockquoted callout written during M005 — "**A reordered prompt
that was mid-delivery may come back at the end.**" — which ends with the instruction
*"if you want a prompt to go next, add it last rather than reordering an older one."*

That is a workaround for this defect, presented to users as the way to use the feature.
When C001 lands the **whole callout** is removed, not merely its last sentence: after the
fix the premise ("it resumes at the end of the queue") is false, and leaving the framing
would teach a user to work around a bug that no longer exists.

### 3.4 Two M005 tests assert the defect

`user_prompt_queue_a_released_delivery_returns_to_the_back_of_the_queue` and its
in-band assertion encode the broken behaviour. They were written as **measurements of an
accepted-and-documented outcome**, not as contract requirements — which is exactly why
they are safe to correct. They must be **replaced** by tests asserting position
preservation, and the replacement must state that it supersedes the measurement. Leaving
them would block the fix and leave a green suite describing a bug as correct.

## 4. Invariants that must not regress

1. **`UNIQUE (session_id, ordinal)` holds at every point of every write**, including
   mid-transaction. The displacement step is what guarantees this; it may not be
   simplified away.
2. **Reorder remains collision-free and total** — M002b's contract.
3. **The client's permutation contract does not change**: a client still sends ids for
   the **pending** rows only. This fix must not push the obligation onto the client.
4. **At-most-once delivery** and the `(session_id, turn_id)` reaper behaviour are
   untouched.
5. **An entry interrupted mid-admission is still reconciled by receipt status**, not by
   ordinal.
6. **Terminal rows are never delivered and never ordered into the user's queue.**
7. M005's frontend contract — the box renders the daemon's ordinal and never renumbers
   locally — is unchanged.

## 5. Scope

### In scope

- `renumber_dense` and its two callers (`reorder`, `delete` survivor renumbering).
- The merge rule that decides where non-pending live rows land (§6).
- The three return paths, **only to the extent they need no change** — under Option A
  they need **none**, which is a large part of why it is recommended.
- Replacing the two tests that assert the defect.
- Removing the `docs/tui.md` callout at `357-362` in full.

### Explicitly out of scope

- **No migration.** (Options B/C are recorded in §6 only to show what was rejected.)
- **No new status value**, no change to v80/v81.
- **No change to the delivery pump, its callers, or the callsite guard.**
- **No change to M006's settlement transition** (a steered entry leaves the queue; it
  never returns, so it is not affected).
- **No change to the projection DTO** — `ordinal` keeps its meaning.
- **No frontend change**, unless a test proves one is needed.
- **No rewrite of M002b's or M003's closure records.**

## 6. The decision

**Option A — renumber the whole *live* set, preserving non-pending rows at their
existing rank. (selected)**

In `renumber_dense`, the write set becomes **every live row** (pending **and**
delivering), not just the client-named pending rows:

1. Displace **every row in the session** exactly as today. This must keep including
   terminal rows — they are not in the live set but they still occupy ordinals under a
   session-wide `UNIQUE`, so excluding them from the displacement would reintroduce the
   collision the offset exists to prevent. **This step is unchanged and is the reason the
   option needs no migration.**
2. Compute the new live order: take the client's pending order, then re-insert each
   non-pending live row at **its existing rank among live rows**.
3. Write dense `0..n-1` for that merged list.

Because a `delivering` row now *receives* a dense ordinal inside the band, every one of
the three return paths in §3.1 lands back at its correct position with **no change to
those functions at all**. It also removes M005's cosmetic symptom — the delivering row
no longer renders last — as a side effect rather than a goal.

Worked example: live = `[p1(0), d1(1), p2(2)]`, client reorders pending to `[p2, p1]`.
`d1` keeps rank 1, so the new order is `[p2(0), d1(1), p1(2)]`. If `d1` is released it
returns `pending` at ordinal 1 and is delivered after `p2` and before `p1` — which is
where the user put it.

**Merge rule (decided): preserve existing rank.** A non-pending live row keeps its
position among live rows; it is never pinned to the top or the bottom. Pinning is the
alternative a reviewer may prefer, and it is a one-line change to the same merge step —
but it would move a released entry to an end, which is the defect under repair. If the
maintainer prefers pinning, change it here before implementation and record it; do not
discover it in the code.

**Option B — a second durable order column.** Add `order_key` for the user's position
and keep `ordinal` as a pure write fence. Also correct, and it needs **v83**, a second
source of truth, and a change to the read projection. Rejected because it is strictly
larger for the same outcome, and because v82 belongs to M007.

**Option C — re-densify on return, per path.** Add an ordinal rewrite to
`release_delivery`, `retry_failed`, and `resolve_stranded_delivery`. Rejected: it cannot
restore a *position*, because the pre-displacement ordinal is not recoverable — the
displacement is arithmetic and a second displacement makes the original ambiguous. It
would fix "outside the band" while still delivering the entry at an arbitrary position.
This is the option a careless fix reaches for, which is why it is written down.

## 7. Ordered work packages

### Work package A — The merge step

*Intent:* one function that computes the merged live order from the client's pending
order and the live rows' existing ranks.

*Acceptance evidence:* unit tests for: no non-pending rows (behaviour identical to
today); one non-pending row at head, middle, and tail; a `delivering` row that is
claimed concurrently with the reorder.

### Work package B — `renumber_dense` and its callers

*Intent:* write the merged list densely; keep the displacement step exactly as it is.

*Acceptance evidence:* M002b's uniqueness tests pass **unmodified**, including
`..._reorder_with_a_delivering_row_keeps_ordinals_unique`; a new test asserts every live
row ends inside the band.

### Work package C — Correct the tests that assert the defect

*Intent:* replace the two M005 tests with position-preservation tests.

*Acceptance evidence:* a new test asserting a released entry resumes **at its prior
rank** with the other rows' relative order intact; the M005 control test
(`..._a_release_without_a_reorder_keeps_its_place`) passes unchanged; a comment in the
replaced tests states they superseded a measurement of an accepted-and-documented
outcome.

### Work package D — Docs

*Intent:* remove the `docs/tui.md` callout at `357-362` in full.

*Acceptance evidence:* `grep` proves the callout and its "add it last" instruction are
gone; `docs/tui.md` no longer tells a user to avoid reordering, and the surrounding
"Reordering" section still describes `[`/`]` accurately.

### Work package E — Proof the guard holds

*Intent:* prove the new merge is doing the work.

*Acceptance evidence:* two injections — (i) replace the merge with "pending targets
only", the old behaviour, and the position test must fail; (ii) drop the displacement
step, and the uniqueness tests must fail. Both reverted afterwards.

## 8. Failure, cancellation, restart, and contention semantics

- **Reorder racing a claim:** the merge reads live rows inside the same transaction as
  the write, so a row that becomes `delivering` mid-reorder is either in the merged list
  or was never read; it is never displaced and forgotten. This is the case most likely to
  be implemented wrongly and needs a test that drives it deliberately.
- **Claim racing this fix's write:** the `status = 'pending'` fence is unchanged, so the
  at-most-once guarantee is untouched.
- **A returned entry and a concurrent delete:** delete's survivor renumbering now covers
  the returned entry, so it is re-densified rather than stranded — the old behaviour is
  what left it out of band.
- **Restart:** no recovery path changes. Reconciliation still runs by receipt status; a
  stranded `delivering` row released at startup now resumes in place.
- **Terminal rows:** never in the merged live list, so they can be displaced without ever
  appearing in the user's queue.

## 9. Compatibility and migration

**None.** No schema change, no layout bump, no version change. `STORAGE_LAYOUT_VERSION`
stays **81**. v82 remains M007's.

This is the whole reason Option A was selected over B: the defect does not require a
migration to fix, and taking v83 to add `order_key` would have been permanent cost for a
temporary mechanism.

## 10. Required tests

### Property test (this is an invariant milestone)

- **Position preservation:** for any interleaving of reorder / delete / claim / release,
  every entry that returns to `pending` resumes at the rank it held before the
  displacement, and the relative order of the other live rows is unchanged.

### Regression tests

- The two M005 tests replaced as described in §7/C.
- M005's control test passes unmodified.
- M002b's reorder/delete round-trip and uniqueness tests pass **unmodified**.
- M003's delivery tests pass **unmodified**, including the release path.

### Guard non-vacuity

- Both injections in §7/E, reported and reverted.

## 11. Required verification commands

```bash
cargo test -p codegg-core --test user_prompt_queue
cargo test -p codegg-core --test user_prompt_queue_mutation
cargo test -p codegg-core --test user_prompt_queue_delivery
cargo test -p codegg --lib -- dialogs::queue commands::queue
cargo test --test user_prompt_queue_box

python3 scripts/check_queue_delivery_callsites.py
python3 scripts/check_project_catalog_invariants.py

cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

`CARGO_BUILD_JOBS=2`. Never `--all-features`.

## 12. Documentation updates

- `docs/tui.md` — remove the "add last rather than reorder" workaround.
- `architecture/session.md` + the `session-storage` skill — the renumber rule and the
  merge rule, including **why non-pending live rows keep their rank**.
- `plans/subsystems/user-prompt-queue-roadmap.md` §7 M005, §10, and §12 — record that
  the accepted-and-documented disposition was superseded by this corrective pass, by
  **annotation**, not by rewriting the closed records.
- `plans/registry.md` — C001 row, and the M005 blocker note updated to name C001 as its
  resolution.

## 13. Acceptance criteria

1. An entry whose delivery is released resumes at its own position, not at the back.
2. The same holds for `retry_failed` and for startup reconciliation of a stranded
   entry.
3. `UNIQUE (session_id, ordinal)` holds throughout; M002b's tests pass unmodified.
4. A `delivering` row no longer renders last while a reorder happens.
5. The client's permutation contract is unchanged — still pending ids only.
6. The two M005 tests that asserted the defect are replaced by tests asserting position
   preservation.
7. The `docs/tui.md` callout (`357-362`) and its "add it last" instruction are gone.
8. No migration, no layout change, no status change.
9. Both guards in §10 are proven non-vacuous by injection, and the injections are
    reverted.
10. `scripts/verify.sh quick`, clippy `-D warnings`, and `cargo fmt --check` pass.

## 14. Stop conditions

Report rather than improvise if:

- **The merge rule turns out to need per-client guidance** (for example, telling the
  client where non-pending rows go). That changes the permutation contract and is a
  protocol decision, not an implementation detail — stop.
- **`retry_failed` or `resolve_stranded_delivery` turn out to need ordinal writes** after
  Option A lands. That would mean the merge is incomplete; report which path and why
  rather than patching the path.
- **A second displacement appears to be needed** to keep UNIQUE. Stop: the displacement
  must remain a single uniform step over every session row.
- **The fix appears to require a second ordinal column.** That means Option A failed and
  Option B becomes a maintainer decision, not an implementation choice.
- **Anything suggests editing v80 or v81.** Shipped migrations are never edited.

## 15. Closure evidence required

- Requirement-to-evidence matrix over the ten acceptance criteria.
- The property test's actual output, not a description of it.
- Real command output with counts for §11, labelled local.
- Both injection transcripts.
- An explicit statement of which previously-accepted-documented behaviour was
  superseded, naming the roadmap annotations made.
- A registry unblock audit; note that this unblocks **nothing new** — M006 and M007 were
  already unblocked — but it removes a documented workaround from `docs/tui.md`.

## 16. Handoff notes

This is a small change to one function with a large effect on what users experience.
The risk is not in the write; it is in **two places**:

1. Dropping the displacement step, because it looks redundant once the merge covers
   every live row. It is not — terminal rows still occupy ordinals.
2. Letting the merge reorder non-pending rows as a side effect of the client's
   permutation, which would make a `delivering` prompt jump around on every reorder.

Do not touch the frontend for this. The box is already correct: it renders the daemon's
ordinal and never renumbers locally, which is why this was always a store defect.

Finally: M005's closure record is right. It measured the behaviour accurately and
disposed of it honestly within its scope. This corrective pass does not contradict that
record — it answers the maintainer decision the record asked for. Annotate; do not
rewrite.