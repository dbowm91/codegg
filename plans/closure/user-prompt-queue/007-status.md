# M007 — Delayed queue delivery: closure record

Status: **closed**

Date: 2026-10-18

Milestone: M007 of `plans/subsystems/user-prompt-queue-roadmap.md`
Plan: `plans/implementation/user-prompt-queue/007-delayed-queue-delivery.md`

Closure records are historical evidence. This one annotates what happened; it
does not revise it. M001–M006's and C001's records are untouched.

## 1. What shipped

A user can leave an instruction for later. In `/queue`, `a` and `e` now advance
from the prompt text to a **duration** field; a duration becomes an absolute due
time; a prompt with a due time is not delivered before that instant, and the box
shows which prompts are waiting for a clock and which for a turn.

- `run_at_ms INTEGER`, **nullable, no `CHECK`, no sentinel** — migration **v82**.
  `STORAGE_LAYOUT_VERSION` is 82. v80 and v81 are closed and were not edited.
- Eligibility is one shared predicate, `(run_at_ms IS NULL OR run_at_ms <= ?)`,
  used by `claim_next_for_delivery` and `sessions_with_due_entries` alike.
- Ordering stays `ORDER BY ordinal ASC`, so reorder permutes only entries that
  are **already eligible**.
- A due time at or beyond the entry's own expiry is **refused**, never clamped.
- A **bounded queue-scoped advance tick** wakes the delivery path.

## 2. The decision this milestone was blocked on

The plan's §2 carried a decision brief. It was **taken: Option A**, recorded in
`plans/adrs/ADR-0039-queue-advancement-wakeup-authority.md`.

**Status: proposed, not accepted.** Acceptance is a maintainer action per the
plan's §14, and this record does not perform it. The implementation was
authorized by the decision, not by the ADR's existence.

Why A over the alternatives:

| Option | Why not |
|---|---|
| A. bounded queue-scoped tick | **Taken.** Only option that delivers the promise without coupling to an unrelated subsystem. |
| B. reuse the 300 s projection tick | Wrong owner (projection retention), conditionally spawned behind a feature flag, 5-minute granularity. |
| C. accept late delivery | Zero machinery, but "1 hr" becomes "1 hr, or whenever this session next does something". |

## 3. The roadmap premise was falsified, and the exit condition could not stand

The roadmap scoped this milestone against "an existing periodic observation
point". Measured during planning, then re-measured against this baseline:

- `src/core/daemon_construct.rs:163-176` — the only queue-adjacent periodic loop
  is a 300 s projection **retention** tick, spawned only when the projection seam
  is enabled.
- `scripts/check_queue_delivery_callsites.py:42-45` — exactly two permitted
  callers of the delivery entry: the post-terminal wake and the restart twin.
- Neither fires for an idle session holding a scheduled entry.

So M007's exit condition — "advancement introduces no new timer thread or wakeup
source" — **cannot be met as written**, and meeting the milestone's actual value
requires exactly one new wakeup source. The roadmap, §5 and §7, now say what the
implementation really guarantees: the wakeup **parks with no timer armed** when
nothing is scheduled.

## 4. Verification defect found in this milestone's own plan

The plan's §3 measured `parse_duration` and reported its grammar as
`30s`/`5min`/`10m`/`2h`/`1d`, noting that the roadmap's own example spelling
`1 hr` **does not parse**. Re-measured at this baseline
(`src/background_task_migration.rs:11-34`) — confirmed exactly. The plan's
warning was correct and the copy rule follows it: **`1h`, never `1 hr`**.

## 5. Deviations from the plan, and why

**The authoring surface is a second step, not an inline suffix.** The plan left
the capture mechanism open. A delimiter design (`ship the docs | 1h`) was
rejected: prompt text is user data the box must not reinterpret, and any
delimiter is a character the user cannot type, so `ship the docs | 1h` and
`ship the docs | 1h | check the 1h build` are not distinguishable. `a`/`e`
advance from text to duration instead, which also makes an existing schedule a
first-class editable field.

**Delivery is a floor, not a promise.** Stated as such in `docs/tui.md`,
`architecture/core.md`, and ADR-0039. A due time that passes while the daemon is
down is honoured **late** on the next boot. Copy asserts no wall-clock
precision, and `user_prompt_queue_copy_never_promises_a_promised_delivery_time`
asserts the forbidden words absent.

**No `CHECK` on the new column.** A nullable column with no sentinel admits
every value a writer can produce, so there is no invalid state to constrain.
This is structural, not a shortcut: v80's `status` `CHECK` cannot be `ALTER`ed by
SQLite without a rebuild, so a "scheduled" status was never available additively.

**The notify fires unconditionally on add and edit**, not only when a schedule is
set. Clearing the last schedule must also wake the tick so it can go back to
parking, and tracking the prior value would mean reading the row the store has
already replaced. The cost is one indexed `MIN()` per mutation.

## 6. A real defect the milestone's own wakeup test found

**The tick spun at full speed while a due entry stayed undeliverable.**

An entry whose due time has passed but which cannot be delivered — its session
is busy, its owner is gone, composition is refused — stays `pending` and
**permanently due**. The loop computed `wait = (due - now).max(0)`, re-armed at
zero, and consumed a core indefinitely. Observed directly while diagnosing a
test failure: repeated `sleep fired, advancing` with no state change.

Fixed with `SAME_DUE_RETRY_FLOOR_MS` (30 s), a damper on retrying **the same
due instant**. It is deliberately not a poll interval:

- a due instant the task has **not** just tried still fires immediately, which is
  the restart case;
- the post-terminal wake and the restart twin are untouched, so anything that can
  actually resolve a stuck entry still resolves it promptly;
- the task still parks with no timer when nothing is scheduled.

## 7. A stale assertion from M006, surfaced here

`user_prompt_queue_empty_state_names_add_and_nothing_it_cannot_do` forbade the
string `"send now"` anywhere in the rendered box. M006 added `s send now` to
`HINT_BROWWSING`, so the assertion had been failing since M006 — the test is
in `src/tui/components/dialogs/queue.rs` and M006's verification ran
`tests/user_prompt_queue_box.rs` and clippy, not this lib target.

Fixed by correcting the list to what the box still **cannot** do (`adopt`,
`transfer`, `timer`, `later`) and asserting the implemented verbs are advertised.
This was a real defect that would have reddened CI, not a cosmetic one.

## 7a. Two more real defects, found by the full workspace suite

M006's verification ran the box tests and clippy but not the whole lib target,
so M004's `/queue` command left **three** tripwires failing for days:

- `architecture/command.md` documented **164** built-ins across six places
  while the registry held 165. Guard: `command_docs_count_matches_registry`.
- `built_in_command_count_matches_release_docs` pinned 164 in `src/tui/command.rs`.
- `every_builtin_action_is_referenced_by_a_canonical_command` compared the
  registry against an exhaustive `BuiltinSlashAction` list that never gained
  `Queue` — so the test's own "no surplus" arm could not see it.

All three are fixed: the count moved to 165 in the doc and in the pin, `Queue`
is in the exhaustive list, and `/queue` now has a table row in
`architecture/command.md`. Its registered **description** was also stale — it
still said "j/k move, r refresh, d delete", omitting M005's add/edit/reorder and
M006's send-now — and now names the whole keymap.

**The lesson is the one M006 already recorded and then repeated.** A milestone's
verification is only as wide as the commands it chose to run; `verify.sh quick`
does not execute workspace tests, and the lib target was never in M006's sweep.

## 7b. Pre-existing environment failures on this host

The full workspace suite (`cargo nextest run --workspace --locked --profile ci`)
reports 14,606 tests with **75 failures and 1 timeout**, of which only the three
above are attributable to this workstream. The remaining ~72 share one cause and
one shape:

    MissingWorkspaceRoot { context: "managed shell dispatch has no canonical workdir" }
    Execution("no workspace root was resolved for this call (the tool was
              constructed without a workspace root); refusing to substitute the
              process working directory)")
    workspace register failed: "workspace root '/tmp' must be an absolute,
                                non-symlink path"
    test workspace registration: InvalidRootPath("/tmp")

Those tests hard-code `/tmp` (or a `TMPDIR` temp path) as a workspace root, and
**`/tmp` is a symlink to `private/tmp` on this macOS host**. The root validator
rejects symlinked roots by design — that refusal is the AGENTS.md
"missing workspace root is a refusal, never a cue to read the process cwd" rule
working as intended. Every failing file (`tests/lsp.rs`, `tests/tool_execution.rs`,
`tests/workspace_isolation.rs`, `tests/session_family/`, and others) is **absent
from this change's diff**. These are expected to pass on Linux CI, where `/tmp`
is a real directory.

## 8. Guards

`scripts/check_queue_delivery_callsites.py`:

- `PERMITTED_CALLERS` widened from two to three, with the rationale on the
  constant. A third caller is a recorded decision under `AGENTS.md`, never a
  quiet third call site.
- `--self-test` now injects a **fourth** caller and proves it is still rejected,
  and that the unmodified tree still passes. Widening a guard without re-proving
  its refusal constrains nothing.
- Module docstring corrected from "three call sites — the post-terminal wake and
  the bounded restart twin" (which named two) to name all three.

`scripts/check_project_catalog_invariants.py` caught a **second** wiring site on
first run: the `migrate_and_record` upgrade chain is separate from the dispatch
arms, so adding v82 required three edits, not two.

## 9. Test inventory

| Suite | Before | After | New |
|---|---|---|---|
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | 26 | 38 | 12 |
| `cargo test --test user_prompt_queue_box` | 61 | 64 | 3 |
| `cargo test --lib dialogs::queue` | 44 | 55 | 11 |
| `cargo test --test identity_m003_daemon_authorization` (queue subset) | 13 | 16 | 3 |
| `cargo test --test storage_migrations` | 4 | 5 | 1 |

Revisions to existing tests, each annotated in place:

- **M005** `add_commits_text_without_an_entry_id`,
  `identical_text_is_not_a_duplicate`, `edit_commits_the_same_entry_id` — one
  `Enter` now advances; the same contract is asserted one step later.
- **M006-era** `the_unowned_refusal_says_where_the_repair_is_not` and the hint
  test — the stale `"send now"` ban (§7).
- **Three `STORAGE_LAYOUT_VERSION` tripwires**, all pinned to `81` because that
  was the current number so a *silent* bump would be visible in review. M007
  legitimately took v82, so each was moved to `82` with the reason in place:
  `user_prompt_queue_v81_migration_creates_the_owner_table` and the pin in
  `user_prompt_queue` both keep their tripwire function intact.
- **One tripwire could not be moved, because it was structurally impossible.**
  `user_prompt_queue_v80_migration_creates_table_index_and_layout` asserted
  `run_at_ms` must not ship in v80. That pool is fully migrated, so
  `pragma_table_info` reports the *union* of every migration — once v82
  legitimately adds the column, nothing there can distinguish v80's DDL from
  v82's. Replaced with an assertion of what is still observable (the column
  exists, `INTEGER`, nullable) plus a pointer to
  `v81_to_v82_adds_run_at_ms_as_null_not_a_sentinel` in
  `tests/storage_migrations.rs`, which stages v81 and runs only v82 and so
  **does** observe provenance. The guard was relocated, not dropped.
- `tests/user_prompt_queue_box.rs` gained a `commit_compose` helper so the
  two-step flow is collected in one place rather than eleven scattered extra
  keystrokes that each read as a magic key.

### A test that asserted the wrong thing

`user_prompt_queue_advance_task_parks_when_nothing_is_scheduled_and_wakes_on_a_new_schedule`
initially failed. The cause was **not** the wakeup: delivery claims the
**lowest-ordinal eligible** entry, and the park fixture's immediate row sat at
ordinal 0, so the tick correctly claimed *that* row and the scheduled row was
never reached. The test now deletes the park row before scheduling, with a
comment saying why — leaving it in place would assert the wrong thing while
looking like it passed for the wrong reason.

## 10. Findings carried forward, not fixed

**The M003 lock-upgrade deadlock is unchanged.** The maintainer decided to land
it and file a corrective rather than widen M007 into a store-wide
transaction-shape change. Its roadmap §10 row stands as written by M006:
`SQLITE_BUSY` (code 6) on `begin()` → `SELECT` → `UPDATE`, `busy_timeout` does
not retry it, and the at-most-once fence is unaffected. M007 added no
concurrency test of its own, so it does not change the measured flake rate.

Two further open items are unchanged by this milestone: `retry_failed`'s revive
position and the second-displacement collision (both C001 §10).

## 11. What M007 does not deliver

- **Recurring schedules** and calendar-style triggers — deferred by the roadmap.
- **Wall-clock precision.** A due time is a floor.
- **Delivery success.** No test in this tree reaches
  `QueueDeliveryOutcome::Delivered`; the daemon fixture has no provider
  selection, so composition fails closed and every attempt **releases**. M007's
  daemon tests therefore assert the narrower, load-bearing property: *the tick
  reached the right entry, at the right time, and no other*, using the entry's
  durable `updated_at_ms` — which every claim and release rewrites. This is
  stated at the top of those tests rather than left implicit.
- **ADR-0039 acceptance.** Still a maintainer action.

## 12. Verification

Run locally against the working tree; no hosted CI claim is made or implied.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean, no warnings |
| `cargo check --workspace --all-targets` | clean |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | 38 passed, 0 failed |
| `cargo test -p codegg-core --test user_prompt_queue_mutation` | 34 passed, 0 failed |
| `cargo test -p codegg-core --test user_prompt_queue` | 22 passed, 0 failed |
| `cargo test --test user_prompt_queue_box` | 64 passed, 0 failed |
| `cargo test --test storage_migrations` | 5 passed, 0 failed |
| `cargo test --lib dialogs::queue` | 55 passed, 0 failed |
| `cargo test --test identity_m003_daemon_authorization user_prompt_queue` | 16 passed, 0 failed |
| `check_queue_delivery_callsites` | pass |
| `check_queue_delivery_callsites --self-test` | pass — fourth caller rejected |
| `check_project_catalog_invariants` | 7/7 pass |
| `check_audit_coverage` | pass |
| `check_adr_integrity` | pass |
| `scripts/verify.sh quick` | pass |

`scripts/verify.sh quick` does not run workspace tests, so the M003 deadlock
will surface in CI rather than here (§10).