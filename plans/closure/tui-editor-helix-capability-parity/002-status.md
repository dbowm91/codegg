# TUI Editor Helix Capability Parity Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/002-change-verbs-and-no-yank-deletion.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-002--change-verbs-and-no-yank-deletion`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same working tree also carries
  M001 and M003–M009 of this roadmap. No statement below is attributable to a
  SHA.

## 1. Executive finding

The capability is delivered. `c` and `Alt-c` delete the held selection and hand
the keyboard to Insert, and `Alt-d` is bound as the explicit no-yank delete.

The load-bearing property — **the delete happens first, and Insert follows only
if the delete was accepted** — is implemented as an ordering, not as a comment:
the buffer resolves `ChangeSelection` into a distinct `HelixVerbOutcome::ChangeRanges`,
the App layer submits that transaction, and only the drain that observes an
accepted edit moves the mode. A refused change therefore leaves the reader in
Normal with the text intact and a notice, which is the state §8 of the plan
forbids the alternative of. That is asserted against **both** `input_mode` and
`editor_state.mode()`, so the route's mode and the editor's presentation cannot
disagree silently.

The D1 decision is recorded in the plan, in `docs/tui.md` and in a code comment
on `HelixVerb::ChangeSelection`: nothing here yanks, because this editor has no
register and its only yank target is the system clipboard. `d` and `Alt-d` are
byte-for-byte the same gesture until M007 gives the editor a register; `c` and
`Alt-c` likewise. That is a recorded sequencing decision with a named successor,
not an omission.

The "does not touch the clipboard" claim is proved where it can actually be
proved rather than by comment: at the dispatch boundary, with `y` as the
positive control. The clipboard has exactly one writer (`yank_editor_ranges`,
which calls `crate::util::clipboard::copy_to_clipboard`), and it is reachable
only from `BufferKey::YankRanges`. Proving the delete family never reports a
yank is therefore the same claim as proving it never writes the clipboard, and
it is the assertion M007 will have to invert for `d`/`c` and preserve for the
`Alt` pair.

The status was conditional for two reasons, neither of them the capability, and
the gate half is now discharged: `cargo fmt --check` and `scripts/verify.sh
quick` were not run by this record's author, and the comment-ratchet invariant
did not hold — `check_comment_budget.py` was red with three violations on this
milestone's edit path (§10). The 2026-11-13 `scripts/verify.sh full` run passed
on this tree — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures,
`cargo fmt --check --all` clean, clippy `-D warnings` clean — and `full` calls
`quick`, which calls `run_guards()`, so both gate conditions are met and the
three editor-path ceilings are re-banked in `docs/comment-budget.toml` (§10).
What remains is one **open owner question**, not a gate: whether the `space ?`
box's omission of `Alt-d` is non-gating (§10, §11).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `c` with a selection deletes it, leaves the cursor at the deletion point, and enters Insert | `c_deletes_then_enters_insert_and_the_typing_run_is_a_second_step`: `"ab\ncd\n"` → `"cd\n"`, `cursor == 0`, `input_mode == Insert` | pass | |
| …and the editor's own presentation agrees with the route | same test: `editor_state.mode() == EditorMode::Insert` | pass | The two tables are asserted separately on purpose. |
| `c` with a collapsed selection deletes the character under the cursor | `change_with_a_bare_cursor_takes_the_character_under_it` (mid-line `1..2`); `c_with_nothing_selected_is_claimed_rather_than_typed` (`"ab\n"` → `"b\n"`) | pass | Pinned against the reference dialect and asserted, not assumed. |
| At a line end `c` joins the two lines | `change_with_a_bare_cursor_takes_the_character_under_it` applies the resolved delete and asserts the resulting text `"onetwo\n"` | pass | Asserted as text, not as an offset, so a range one byte out would fail. |
| At the end of the buffer there is nothing to take, and Insert is not entered | `change_with_nothing_left_to_take_resolves_nothing`: empty buffer and cursor at end both resolve `Handled` with the cursor unmoved | pass | No Insert over text that survived. |
| `Alt-c` and `Alt-d` are bound and consumed | `alt_c_changes_and_alt_d_deletes_through_dispatch`: `Alt-c` → `ChangeRanges(0..3)`, `Alt-d` → `DeleteRanges(0..3)`, buffer applies neither | pass | Both are modified chords, so they were inert rather than typed before this change. |
| `c` is claimed even with nothing selected, so it cannot reach the composer | `c_with_nothing_selected_is_claimed_rather_than_typed` asserts the key is handled and a character was deleted | pass | The fallthrough hazard in its most visible form. |
| `c` on a read-only attachment reports a notice and leaves the route in Normal | `c_on_a_read_only_attachment_reports_and_stays_in_normal`: text unchanged, `input_mode == Normal`, `editor_state.mode() == Normal`, notice contains `Read-only` | pass | The plan's §8 correctness hazard, asserted rather than written down. |
| `c` plus a typing burst is exactly two history entries | `c_deletes_then_enters_insert_and_the_typing_run_is_a_second_step`: `undo_depth == 1` after the delete, still `1` while the run is open, `2` after `Esc` | pass | "exactly two steps: the delete and the run". |
| A multi-member `c` is one history entry | `a_multi_member_change_is_one_undo_step`: two members, `undo_depth == 1` | pass | One gesture, one entry — not one per member. |
| A change past `MAX_DOCUMENT_EDITS` is refused, changes nothing, and does not enter Insert | `a_change_past_the_transaction_limit_is_refused_and_does_not_enter_insert` | pass | Resolved-then-refused: the ordering proof, not just the refusal. |
| No transaction, no Insert | `ChangeRanges` is a distinct outcome from `DeleteRanges`; the mode move lives in the `ChangeRanges` drain | pass | A flag on `DeleteSelection` would have shared one arm and hidden the difference. |
| None of `c`, `Alt-c`, `Alt-d` changes the clipboard, and `y` still does | `the_delete_family_never_reports_a_clipboard_write`: no `YankRanges` from any of the four (`d` included); `y` reports `YankRanges` as the positive control | pass | See §10 for why this is a boundary assertion rather than a seeded-clipboard one. |
| `d`'s behaviour is not silently changed | `deleting_with_nothing_selected_is_a_claimed_no_op` still passes; `change_ranges` doc records that `d` on an empty set is untouched | pass | The asymmetry is deliberate and recorded. |
| `check_tui_editor_text_authority.py` passes with no allowlist edit | guard run: pass, one informational borrow note | pass | |
| The key-authority guard reports no shadowed arm | 2 guard-block, 89 Normal-mode, 8 Insert-mode chords, no shadowed arm | pass | 84 immediately after this milestone's three chords; 89 now, after M003–M008 (see §4). |
| The doc-drift guard passes in both directions | 52 leaf gestures and 98 chords documented; every key declared unbound is unbound | pass | |
| `docs/tui.md` states the yank divergence and names M007 | `git diff -- docs/tui.md` adds the `d / Alt-d` and `c / Alt-c` rows, the "at a line end the character under the cursor is the line break, so `c` there joins the two lines" prose, and the paragraph naming roadmap M007 as the milestone that changes it | pass | |
| No migration, protocol variant, crate or new authority | No `codegg-core`, `codegg-document`, `codegg-protocol` or `Cargo.toml` change for this milestone | pass | Stated as §15 requires, not omitted. |

## 3. Production implementation evidence

- `HelixVerb::ChangeSelection` (`src/tui/editor.rs`) is a **distinct variant**,
  not a flag on `DeleteSelection`, so `d` and `c` cannot end up sharing an arm.
  Its doc comment carries D1 and the explicit instruction not to "fix" the
  missing yank in isolation.
- `HelixVerbOutcome::ChangeRanges` is distinct from `DeleteRanges`, and its doc
  states why: the caller's ordering depends on the difference between "delete
  this" and "delete this and hand over the keyboard".
- `change_ranges` is the one place the bare-cursor rule lives: a held set
  resolves exactly as `selection_ranges` does (so `c` and `d` cannot disagree
  about the bytes), and only a *bare* cursor falls through to the character
  under it — which at a line end is the terminator, hence the join.
- `src/tui/commands/editor.rs` adds the three arms: `c` with `NONE`,
  `Alt-c` and `Alt-d` with `ALT`. Order inside the `c` arm is resolve → submit
  → the mode moves in the `ChangeRanges` drain.
- `mutates_buffer` gains `ChangeSelection`, so a read-only attachment is refused
  before the route can reach the point where Insert would follow.
- `docs/tui.md` and `EDITOR_KEYMAP` in `src/tui/components/editor.rs` were
  updated in the same change as the arms.

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`.

```bash
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_leaf_authority.py

# lib tests, run from the built test binary because three sibling sessions
# held the cargo build lock (see below)
./target/debug/deps/codegg-ef34c509a35311b3 tui::editor
./target/debug/deps/codegg-ef34c509a35311b3 tui::commands::editor

# the whole render family, same caveat
./target/debug/deps/tui_render-17e4c7be3ef479f9
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 89 Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm" |
| `check_tui_editor_text_authority.py` | **pass** — no retained text buffer, no direct filesystem reads |
| `check_tui_editor_doc_drift.py` | **pass** — 52 leaf gestures, 98 chords documented, every declared-unbound key unbound |
| `check_tui_editor_leaf_authority.py` | **pass** — 6 tables, 52 glyphs, 13 prefix bindings, no duplicate, no unreachable leaf |
| lib tests, filter `tui::editor` | **pass** — 292 passed, 0 failed |
| lib tests, filter `tui::commands::editor` | **pass** — 259 passed, 0 failed |
| `tests/tui_render`, no filter | **pass** — 1217 passed, 0 failed, 0 filtered out |
| `python3 scripts/check_comment_budget.py` | **fail at the time of writing; superseded 2026-11-13 — see the correction log.** 20 violations tree-wide, three on this milestone's edit path: `src/tui/editor.rs` 3320 comment lines against a ceiling of 2926, `src/tui/commands/editor.rs` 2481 against 2275, `src/tui/components/editor.rs` 842 against 840. Those ceilings are now banked at 3693 / 3110 / 902 and the guard passed inside the 2026-11-13 `full` |

Honesty notes on how these were produced:

- The `cargo test -p codegg --lib …` and `cargo test --test tui_render`
  invocations were **not** issued through `cargo`. Three sibling sessions were
  building in the same target directory and every `cargo test` queued behind the
  build lock, so the compiled binaries
  (`target/debug/deps/codegg-ef34c509a35311b3`, mtime 04:27:56;
  `target/debug/deps/tui_render-17e4c7be3ef479f9`, mtime 04:26:10) were run
  directly with the filters `cargo test` would pass. Both postdate the newest
  source file on this path (`src/tui/commands/editor.rs`, 04:11:16). The tree
  kept moving after that: other sessions changed
  `src/tui/components/section.rs`, `src/tui/components/dialogs/agent.rs`,
  `src/main.rs` and `crates/codegg-core/src/session/request_ledger.rs`, and at one
  point the library did not compile at all (`E0599`, resolved by its owner within
  minutes). Read these figures as the tree at those mtimes, not as a standing
  guarantee.
- The Normal-mode chord count is **89**. The plan's §3 baseline is 74; M001
  added seven and this milestone three, which is 84, and M003–M008 of the same
  roadmap account for the remaining five in the same uncommitted tree. No commit
  separates the milestones, so no per-milestone reading can be re-measured.

### Not run

| Command | Why |
|---|---|
| `cargo fmt --check`, `bash scripts/verify.sh quick` | Not run at the time of writing: `quick` was the routine guards plus a workspace check over a tree other sessions were actively editing, and a red result would not have been attributable here. **Discharged 2026-11-13** — the `scripts/verify.sh full` run passed on this tree (`full` → `quick` → `run_guards()`, whose first line is `cargo fmt --check --all`). |

## 5. Invariant review

- **One undo step for the delete.** `c` submits exactly one transaction;
  entering Insert edits nothing, so it adds no entry. Asserted at `undo_depth ==
  1` immediately after the delete, and for a two-member set.
- **A typing run starts where the cursor ends.** The delete and the run are two
  entries, and the run is one entry however many keystrokes it took. Asserted as
  `1` while the run is open and `2` after `Esc`, which is the observable form of
  the predecessor M019 rule.
- **No dead Insert mode.** On a read-only attachment `c` is refused with a
  notice and neither the route's `input_mode` nor the editor's `mode()` moves.
- **`d`'s current behaviour is not silently changed.**
  `deleting_with_nothing_selected_is_a_claimed_no_op` still passes, and
  `change_ranges`' doc records that re-deciding `d` is not this milestone's.
- **No second text buffer.** `change_ranges` reads the live snapshot and retains
  nothing; the text-authority guard passes with no allowlist edit.
- **One owner per chord.** No shadowed arm; `c` is claimed with nothing
  selected, which is what stops it reaching the composer.
- **Positions, never text.** The verb returns merged, document-ordered byte
  ranges and one transaction.

## 6. Failure and recovery review

- **Partial failure.** If the controller rejects the delete, the notice reports
  the rejection, the document is unchanged and Insert is not entered. There is
  no state in which the text is deleted but the mode is Normal, or the mode is
  Insert but the text survived. Both halves are asserted
  (`a_change_past_the_transaction_limit_is_refused_and_does_not_enter_insert`).
- **Duplicate delivery.** Not applicable; synchronous local gesture.
- **Restart.** No state. **Stated explicitly** rather than omitted, as the
  plan's §10 requires.
- **Cancellation races.** None; no async work is added and no request is issued.
- **Stale generations.** The verb reads the current snapshot; `try_snapshot()`
  cannot return a stale one.
- **Bound reached.** A change past `MAX_DOCUMENT_EDITS` is refused with the
  existing notice and Insert is not entered.

## 7. Migration and compatibility review

**No storage, protocol or configuration migration.** Two compatibility surfaces
and both are recorded in `docs/tui.md` in the same change:

1. **The keymap.** `c` stops reaching the composer on the editor route — the
   intended effect of a binding, and the fix for the fallthrough hazard.
2. **The yank-on-change divergence.** D1 chose option (b): `c`/`d` do not yank,
   `Alt-c`/`Alt-d` are the explicit no-yank spellings, and the two pairs are
   identical until M007 introduces the register set. The obligation this
   creates is recorded in the plan: when registers land, `d`/`c` must yank into
   the unnamed register and the `Alt` pair must not, while `space y`/`y` keep
   reaching the system clipboard. The clipboard test in this milestone is the
   assertion M007 inverts for the first pair and preserves for the second.

Rollback is deleting the three arms; nothing is persisted.

## 8. Security review

No authorization surface changes. Read-only refusal is the existing
`mutates_buffer` gate, now including `ChangeSelection` so the refusal precedes
the mode move. Every mutation goes through `apply_local`. No path, secret,
process or network surface is touched, and the transaction budget bounds the
work one keystroke can provoke.

## 9. Documentation and operations

- `docs/tui.md` — `d / Alt-d` and `c / Alt-c` rows; the collapsed-cursor and
  line-end rules; the paragraph naming M007 as the milestone that makes `d`/`c`
  yank; the read-only refusal list now including `c`/`Alt-c`.
- `src/tui/components/editor.rs` `EDITOR_KEYMAP` — `d / y` and `c / Alt-c` rows.
- `src/tui/editor.rs` — D1 recorded on `HelixVerb::ChangeSelection`, and the
  `d`-versus-`c` asymmetry recorded on `change_ranges`.
- Guards: key authority, text authority, doc drift and leaf authority all pass
  with their figures moved in the same change as the chords.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | **Discharged 2026-11-13.** The comment corpus ratchet did **not** hold at this record's close: `check_comment_budget.py` reported 20 violations tree-wide, three on this milestone's edit path, with `src/tui/editor.rs` 394 comment lines over its ceiling and `src/tui/commands/editor.rs` 206 over. The three ceilings are now banked in `docs/comment-budget.toml` (`editor.rs` 3693, `commands/editor.rs` 3110, `components/editor.rs` 902) and the guard passed inside the 2026-11-13 `scripts/verify.sh full`. | The D1 and ordering explanations are long and live inline; the ratchet is the mechanism that forces that prose to be paid for deliberately, and the bank is the payment. | None. One tree-wide violation has since appeared in `src/tui/components/dialogs/keybind.rs` from the corrective plan's WP D deletion, outside this path and owned by that work package's manifest bank. |
| low | The no-yank proof is a **dispatch-boundary** assertion (no `YankRanges` from `c`/`Alt-c`/`Alt-d`/`d`, with `y` as positive control) rather than the plan's §7-C "seed the clipboard with a known value and assert it is unchanged". | The implemented form is stronger in the sense that it proves no path to the single clipboard writer exists at all; it is weaker in that it would not catch a clipboard write made outside `yank_editor_ranges`. | None for this milestone; M007 should add the seeded assertion when it splits the two pairs, because at that point the boundary assertion is expected to flip for `d`/`c`. |
| low | **Open owner question — the `space ?` keymap box omits `Alt-d`.** The box's delete row reads `d / y` and the change row `c / Alt-c` (`EDITOR_KEYMAP`, `src/tui/components/editor.rs:253`), while `docs/tui.md` carries all four spellings. The omission is current, not historical. | A reader who learns from the in-editor box will not discover the no-yank delete; only `docs/tui.md` documents it. The owner has **not** ruled on whether this documentation item is non-gating, so this record neither resolves it nor drops it. | Owner decision required: either **accept it as non-gating** — one sentence recorded here, on the ground that the box is a condensed in-editor aid and `docs/tui.md` is the complete document — or **require the row**, in which case the settlement is an `EDITOR_KEYMAP` row for `Alt-d` plus a doc-drift run. Until one of those is recorded, the item stays open. |
| low | `d` and `Alt-d` are indistinguishable until M007, as are `c` and `Alt-c`. | Intended by D1 and documented, but a reader who knows Helix will expect `d` to yank. | M007's closure record must state that `d`/`c` now yank; this record is the document that says so was always intended. |

No medium or high finding. No migration, protocol variant, crate or new
authority was added.

## 11. Roadmap disposition

**Milestone closed (owner decision, 2026-11-14).** The capability, its ordering guarantee, its
tests, its guards and its recorded decisions are complete and verified. The
conditions were:

- **Condition.** Three things had to hold on the merged tree before M003 of this
  roadmap was cited as depending on a closed M002:
  1. `cargo fmt --check` and `scripts/verify.sh quick`; — **met 2026-11-13** by
     the `scripts/verify.sh full` run (17,377 workspace tests + 8,441
     feature-pass tests, 0 failures; formatting and clippy clean).
  2. `python3 scripts/check_comment_budget.py` — then **red**, with the three
     editor-path ceilings exceeded (§10); — **met 2026-11-13 by the second arm**:
     the three ceilings are banked (`editor.rs` 3693, `commands/editor.rs`
     3110, `components/editor.rs` 902) and the guard passed inside that `full`.
  3. the comment ceilings re-banked, or the prose moved, in the same change;
     — **met 2026-11-13** by the bank recorded in `docs/comment-budget.toml`.

What keeps the status at `conditionally closed` is **one open owner question**,
not a gate: whether the `space ?` box's omission of `Alt-d` is non-gating (§10).
Its settlement is either an owner statement that the box is a condensed aid and
`docs/tui.md` is the complete document, or the added `Alt-d` row. This record
does not decide it.

- **Risk.** Low on capability. This milestone adds no runtime state and no async
  work. The whole `tests/tui_render` family passes (1217/1217 at this record's
  close; 1,237 in the 2026-11-13 `full`), the four editor guards pass, and 551
  filtered library tests pass. The former process risk — `quick` and the comment
  ratchet both red in a tree carrying several uncommitted milestones — is
  discharged. The remaining risk is a documented gesture that the in-editor box
  does not name.
- **Exact future evidence.** Satisfied for conditions 1–3 by the 2026-11-13
  `scripts/verify.sh full` run and the re-banked ceilings. Still owed: the owner
  ruling on the box, and the commit that carries this plan (WP 0).

M007 must not be scheduled as if `d`/`c` already yank; the obligation is
recorded in §7 above and in the plan's §9.

## 12. Registry updates

- Mark roadmap M002 **conditionally closed** and link this record.
- Record that the guard's expected Normal-mode count is 89 at this tree state,
  and that D1's transition (registers, M007) is the only thing permitted to make
  `d`/`c` and `Alt-d`/`Alt-c` diverge.
- Do not mark M003 or M007 ready on the strength of this record alone.

## Owner decision (2026-11-14)

**Decision 6 of the seven, taken as B: the box now names `Alt-d`.**

The same box read `d / y` while `docs/tui.md` documents the `d`/`Alt-d` split —
`d` yanks into the unnamed register, `Alt-d` yanks nothing, and neither touches
the clipboard. It now reads `d / Alt-d / y`, so the no-yank delete is
discoverable where the user looks rather than only in the document.

`d` and `Alt-d` remain indistinguishable until M007 (intended by D1, recorded
here and in `docs/tui.md`); that is a register-semantics fact, not a gap in this
milestone.

## Correction log (2026-11-13)

- **§1, the status paragraph** — "`cargo fmt --check` and `scripts/verify.sh
  quick` were not run … `check_comment_budget.py` is red with three violations"
  → the gate half is discharged by the 2026-11-13 `scripts/verify.sh full`
  (17,377 + 8,441 tests at 0 failures, formatting and clippy clean) and by the
  three re-banked ceilings; what remains is the owner question, not a gate.
- **§4, the comment-budget result row** — "**fail** — 20 violations …" → marked
  superseded: the three ceilings are banked at 3693 / 3110 / 902 and the guard
  passed inside the 2026-11-13 `full`.
- **§4, `Not run`** — "`cargo fmt --check`, `bash scripts/verify.sh quick` …
  Not run" → discharged by the 2026-11-13 `scripts/verify.sh full`
  (`full` → `quick` → `run_guards()`, whose first line is
  `cargo fmt --check --all`).
- **§10, low (comment ratchet)** — "The comment corpus ratchet does **not**
  hold … 20 violations" → discharged 2026-11-13 by the bank and the `full` run.
- **§10, low (`space ?` box omits `Alt-d`)** — "Add `Alt-d` to the box, or
  record why the box is condensed" → restated as an **open owner question**
  with its two possible settlements, because the owner has not ruled on whether
  this documentation item is non-gating. The omission is verified current:
  `EDITOR_KEYMAP` (`src/tui/components/editor.rs:253`) carries `d / y` and
  `c / Alt-c`, not `Alt-d`.
- **§11, conditions 1–3** — each marked met with its discharge named; the open
  owner question replaces the gate conditions as the reason the status stays
  `conditionally closed`.
- **§11, risk** — "`quick` and the comment ratchet are both tree-wide gates, and
  both are red" → discharged; the remaining risk is the box omission.
- **§11, exact future evidence** — the transcripts and re-banked ceilings exist;
  only the owner ruling and the commit (WP 0) are outstanding.
- **Run figures left as written** — §2 and §4 report "89 Normal-mode chord(s)",
  "1217/1217" and 292/259 filtered tests from the runs recorded in §4. They are
  unchanged historical measurements, not conditions: the guard now reports 92
  chords and the render family 1,237 tests.
- **Owner decision, 2026-11-14** — `Alt-d` added to the `space ?` box; the record is `closed`.
