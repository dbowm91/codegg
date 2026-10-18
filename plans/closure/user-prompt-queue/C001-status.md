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