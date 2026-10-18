# User Prompt Queue Milestone 005 — Closure Record

Status: **closed**
Date: 2026-10-18
Class: **capability**
Plan: `plans/implementation/user-prompt-queue/005-queue-box-add-edit-and-reorder.md`
Roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Milestone 5)
Baseline: `18c3dca92099fe2de09f875731530348fb76b68f` plus the uncommitted
M001–M004 tree.

## 1. What shipped

M005 makes the prompt queue **authorable and correctable** rather than
append-only. Three actions were added to M004's `/queue` box:

| Key | Action | Request |
|---|---|---|
| `a` | compose and queue a new prompt | `UserPromptQueueAdd` |
| `e` | revise the selected prompt in place | `UserPromptQueueEdit` |
| `[` / `]` | move the selected prompt earlier / later | `UserPromptQueueReorder` |

M004's `j`/`k`, `r`, and `d` are unchanged. Row 0's queue-count suffix is
unchanged.

## 2. Scope boundary held

**Not shipped, deliberately:**

- **Steering / send-now.** M006. No M005 copy says "send now", "force", or
  "steer"; the empty-state and keymap-hint tests assert their absence.
- **Timers / `run_at_ms`.** M007, which owns migration v82.
- **Adoption / transfer.** ADR-0037 keeps them explicit and audited, outside the
  box. See §6 for the dead-key trap this nearly fell into.
- **Dispatch-now.** `a` always queues; M003's wake delivers it.
- **No migration, no protocol change, no store change, no daemon change.** The
  three requests and the response shape already existed from M002b/M003. The
  entire milestone is `src/tui` plus tests and docs.
- **No shared `App`-level `captures_text` predicate.** The plan's stop condition
  holds; `ConnectDialog::is_text_input_step` is the precedent and
  `QueueDialog::is_text_step` follows its shape.

## 3. The answer to WP-D edge (b) — observed, not inferred

The plan required this settled by test. Both halves were measured in
`crates/codegg-core/tests/user_prompt_queue_mutation.rs` against the real
store.

**Question 1 — a `delivering` row's ordinal after a reorder.**
`renumber_dense` displaces every row in the session, status-blind, before
densifying the targets. The displacement is
`USER_PROMPT_QUEUE_RENUMBER_BAND_OFFSET` = 1 000 000.

> **Observed:** after any reorder, a `delivering` row's ordinal sits far above
> every pending row, so it sorts **last** in the projection regardless of where
> the user queued it. The band offset is what keeps
> `UNIQUE (session_id, ordinal)` intact while it is delivering — asserted by
> `user_prompt_queue_reorder_with_a_delivering_row_keeps_ordinals_unique`.

**Question 2 — where the entry lands if that delivery is released.**

> **Observed, and sharper than the plan predicted:** the entry returns to
> `pending` **at the back of the queue** and **keeps the displacement ordinal of
> 1 000 000** rather than being re-densified. A release restores the *status*,
> not the *ordinal*. So the row sits outside the dense band entirely — not
> merely after its neighbours. Verified by
> `user_prompt_queue_a_released_delivery_returns_to_the_back_of_the_queue`,
> which also asserts the other rows stay inside the band.
>
> **Control:** `user_prompt_queue_a_release_without_a_reorder_keeps_its_place`
> shows the entry resumes *in place* when no reorder happened, which is what
> isolates the reorder as the cause rather than the release itself.

**Disposition.** Recorded as accepted-and-documented behaviour. The box renders
the daemon's ordinal verbatim rather than renumbering to hide the gap, and
`docs/tui.md` tells the user to add a prompt last rather than reorder an older
one. Changing `renumber_dense` reopens closed M002b/M003 and is a maintainer
decision, not a TUI workaround — which is the plan's stated stop condition. The
answer is reported here rather than acted on.

M005 neither triggers nor masks this behaviour.

## 4. Verification actually run

All counts are real. `CARGO_BUILD_JOBS=2` throughout; no `--all-features`.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo check --workspace --all-targets --locked` | clean (inside `verify.sh quick`) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **clean, no warnings** |
| `scripts/verify.sh quick` | **passed** |
| `cargo test -p codegg --lib -- dialogs::queue commands::queue` | **48 passed, 0 failed** (6487 filtered out) |
| `cargo test --test user_prompt_queue_box` | **44 passed, 0 failed** |
| `cargo test -p codegg-core --test user_prompt_queue` | **22 passed, 0 failed** |
| `cargo test -p codegg-core --test user_prompt_queue_mutation` | **30 passed, 0 failed** |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | **16 passed, 0 failed** |

Regression targets, all green: `command_routing_adversarial`, `tui_render`,
`tui_main_buffer`, `identity_m003_daemon_authorization`, `tui_presentation_regressions`.

**Not run:** the full workspace test suite, and any hosted CI. Nothing was
pushed. `verify.sh full` was not run.

## 5. Guards proven non-vacuous by injection

Each was reverted immediately after the observed failure.

1. **The text step.** Inverting `is_text_step()` fails
   `user_prompt_queue_text_step_swallows_every_action_letter` with
   `'d' must be text` — the exact failure the milestone exists to prevent.
2. **The pending-only permutation** (§3(a)). Treating every live row as
   operable fails at both levels: the unit test yields `["a","c","b"]` instead
   of `["c","a"]`, and the integration test sends `reorder:a,c,b`, naming the
   `delivering` row and earning `order_mismatch` from the daemon.
3. **The stale-completion guard.** Deleting the three `cancel()` calls in
   `close_dialog`'s `Dialog::Queue` arm fails
   `user_prompt_queue_a_completion_after_close_changes_nothing` with the
   cancelled projection written.

A **fourth injection was attempted and was itself invalid**: the first attempt
replaced the cancels with `begin()`, which bumps the request id anyway, so the
test still passed. That is recorded because it is the same mistake the M004
close found — asserting that a guard works without checking that removing it
changes anything.

## 6. Defects found and fixed during implementation

1. **A live bug: stale completions wrote their projection.** The shared refusal
   helper returned `None` for *both* success and stale/cancelled, so a
   completion arriving after `close_dialog` cancelled its request was treated
   as a success and applied its projection — precisely what the request
   generation exists to prevent. Found by the close-during-mutation test.
   Fixed with an explicit three-state `RefusalOutcome { Applied, Refused,
   Stale }`; every applier now returns on `Stale` without touching anything.

2. **`BufferFull` was outside `is_text_step()`.** The first draft of the local
   compose bound latched a step the text branch could not see, so `Backspace`
   became a no-op exactly when the user needed it to recover. `is_text_step()`
   now covers `BufferFull` too.

3. **`/queue-adopt` was named in user-facing copy and does not exist.** The
   first draft of the `unowned` sentence told the user to "run
   `/queue-adopt` explicitly to claim it". The protocol request exists; **no TUI
   command registers one.** Naming it would have been this milestone's own
   version of the dead-key dishonesty it exists to avoid, so the sentence now
   says the repair is a separate explicit operation outside the box, and a test
   asserts `/queue-adopt` appears in no toast.

4. **Four stale M004 assertions** were inverted or outdated by M005 and had to be
   rewritten rather than deleted: the keymap-hint test forbade `add`/`edit`; the
   empty-state test forbade `add`; the unmapped-codes test asserted that
   `order_mismatch`/`too_many_entries`/`invalid` were unmapped (M005 maps all
   three, so the pin test grew to cover ten codes and the unmapped list shrank to
   `storage_error` plus a synthetic future code).

## 7. Design decisions and their reasons

1. **The text-step guard is structural, not per-letter.** `handle_key` matches
   `is_text_step()` **first, as a separate match** and returns from it, so the
   browsing arm is unreachable while a composer is open. The plan asked for
   `!is_text_step()` on each action letter; that is the shape that eventually
   ships a sixth action with a forgotten guard. Here a new action added to the
   browsing arm cannot be live in a text step at all.

2. **Composed text lives in the widget, not `DialogState`.** There is therefore
   no window in which a later `/queue` can commit text an abandoned step left
   behind — the buffer is dropped by the same unmount that drops the box.

3. **`entry_id` is minted at commit, not when the step opens.** It is the
   protocol's at-most-once key: a retry carrying the same id converges on the
   same row instead of duplicating the user's sentence. An abandoned step
   therefore burns no id.

4. **An edit commits the entry's existing id.** Minting a new one would turn
   "correct this prompt" into "add a second prompt and orphan the first".

5. **`[`/`]` over `Shift+K`/`Shift+J`.** This box already uses `j`/`k` for
   movement, and a shifted variant of the movement key that does something
   *other* than move is a pairing people misfire.

6. **The plan's shifted-key reasoning was a wrong premise.** It concluded
   shifted `K`/`J` survive because `docks_as_viewer()` defaults to `false` — but
   `QueueDialog` **overrides** it to `true`. The real reason is that
   `App::dock_target` has no `DialogType::Queue` arm, so the docking-steal branch
   is dead regardless of the viewer flag. That is now asserted by
   `user_prompt_queue_the_host_does_not_steal_its_keys`, because an omission is
   exactly the kind of property that silently stops holding.

7. **One local refusal, a deliberate exception to M004's rule.** M004 lets the
   daemon refuse `d` on a `delivering` row. Reorder *cannot* follow it: the
   permutation would omit the row, the write would **succeed**, and nothing would
   move — a silent no-op. So `reorder_selected` refuses locally and names which
   end it is at. There is no daemon call that can express "this particular row is
   not yours to move".

8. **A bounds refusal restores the user's text; an ownership or delivery-state
   refusal does not.** `refusal_keeps_text` gates on the three codes whose fix is
   to edit the text. Restoring it for an ownership refusal would suggest a retry
   can work when it cannot.

9. **`order_mismatch` re-reads once and reports.** Never a retry loop: the
   daemon's check is exact-length set equality, so blind retrying against a
   stale projection turns one refusal into a contention source.

10. **The local compose bound sits above the daemon's** (256 KiB vs 64 KiB), so
    the daemon's typed refusal is what a user actually sees; the local bound only
    guards a pathological paste — and it *refuses* rather than truncates, because
    silently shortening a prompt is the one outcome the plan forbids.

11. **Three separate `AsyncUiRequestState` slots**, not one. A delete racing an
    edit must not cancel the edit; a shared slot would make whichever key was
    pressed last the only one that could ever land.

12. **The hint has two forms** and follows the step on screen. Advertising
    `d delete` above an open composer is the same lie as advertising an
    unimplemented key: the user reads a promise the box does not keep.

## 8. Acceptance criteria

| # | Criterion | Status |
|---|---|---|
| 1 | `a` queues, never dispatches, and appears in daemon position | met |
| 2 | `e` changes text, not id or position | met |
| 3 | Reorder moves earlier/later and survives a re-read | met (restart is M002b's already-covered store property) |
| 4 | Reorder succeeds with a `delivering` row present | met — proven non-vacuous |
| 5 | `d`/`e`/`a`/`j`/`k`/`[`/`]` typed into a text step produce text | met — proven non-vacuous |
| 6 | Every advertised key works; the hint names no host-first key | met |
| 7 | Closing with unsaved text adds and edits nothing | met — proven non-vacuous |
| 8 | Bounds and delivery refusals are distinct codes and keep the text | met |
| 9 | Observers refused the box and all three actions | met |
| 10 | No prompt text in any toast, hint, diagnostic, or log | met |
| 11 | No migration, protocol, store, or daemon change | met |

## 9. Residual risks

1. **`renumber_dense` displacement (§3).** A released delivery resumes at the
   back, outside the dense band. Documented, not fixed; the fix is a maintainer
   decision that reopens closed M002b/M003.
2. **`dock_target` has no `DialogType::Queue` arm.** This is what keeps the
   docking steal from swallowing the box's keys. If someone adds that arm, the
   keys silently change behaviour — hence the assertion.
3. **No `reason` field on refusals.** Messages are mapped from stable wire
   codes, so a new daemon refusal code degrades to a verbatim-code sentence
   rather than a tailored one. The fallback is deliberate and tested.
4. **No shared chrome helper.** `QueueDialog` duplicates the established
   per-widget `Chrome { Owned, Hosted }` shape because panel-convergence M003
   deferred a helper and never opened one.
5. **The composed text rides in a `TuiCommand`.** That is the furthest it can
   travel while still being restorable after a bounds refusal. It reaches no
   toast, hint, or log, and the tests assert that.
6. **The pre-existing M003 gap carries forward:** a positive receipt-identity
   test fixture is still absent.

## 10. State handed to M006

M006 (send-now steering) has **no** remaining gate: M004 and M005 are closed and
all four ADRs are accepted. It must not describe its own action as "send now" or
"force" in copy — ADR-0036 fixes the guarantee as *acceptance into the turn's
channel*, not delivery, and `TurnSteer` is best-effort. Nothing in M005's box may
be extended to host it; M006 is a separate surface.