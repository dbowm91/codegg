# TUI Editor Helix Capability Parity Milestone 006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/006-jumplist.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-006--jumplist`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. There is no commit, branch or pull request
  carrying it, and the same tree carries M007 and M008 of this roadmap as well as
  the TUI modularity line.

Labels: **measured** is what this record's author read out of the tree or ran;
**reported** is the implementing session's measurement, not repeated here.

## 1. Executive finding

The capability is delivered as the plan's D1 recommended it: `space j` opens a
**picker** over a bounded, positions-only history, and the two Helix stepping
chords are **not bound**, with the reason recorded where a user reads.

**The store.** `MAX_JUMPLIST_ENTRIES = 32`
(`src/tui/document_session.rs:722`) with a field doc comment that states the
contract rather than describing the type: "Chronological, oldest first, and
deliberately **not** normalized the way [`selections`](Self::selections) is: a
jumplist is a history, so its order is the order things happened … Positions only
— a daemon document identity plus a byte offset, never text — bounded by
[`MAX_JUMPLIST_ENTRIES`], and cleared by the same transitions that clear the undo
history: `clear_history` is the single clearing point". The bound is enforced on
push (`:789`) and disclosed through a fullness predicate (`:820`), and it is
cleared with the rest of the presentation (`jumplist.clear()` at
`editor.rs:7249`, with the comment "The jumplist names positions in the text that
was just replaced").

**The push rule is stated once and is a named-destination rule.**
`is_named_destination` (`src/tui/editor.rs`, immediately after `jumplist_target`)
admits exactly `EditorCommand::BufferStart` and `EditorCommand::BufferEnd`, and
its doc comment names the exclusions: "`j`, `w`, `Ctrl-d`, `gs` and `$` are steps
— relative to where the reader stands or to the line they are on — and recording
each would turn the jumplist into a second, worse undo history." The other
destinations the plan lists (`:goto`, `gd`/`gi`/`gr`, diagnostic stepping,
symbol-picker selection, and the search jumps M003 landed) push through explicit
`note_jumplist_jump` calls in the command layer rather than through that
predicate, which is the right split: the predicate answers "is this motion a
jump?", and the command layer answers "a jump happened here".

**The entry records where the jump started.** `record_jumplist_jump`
(`editor.rs`) takes `before` and returns `None` when
`presentation.cursor_byte == before`, with the reason in its doc comment: "an
entry that recorded where the jump *landed* could not take the reader back … a
`gg` at the top and a `G` at the end are jumps to where the reader already stands,
and an entry for them would be a picker row that does nothing." That is the
plan's work package B acceptance property, implemented as a guard rather than as
a convention.

**An unhonourable entry is refused, never approximated.** `jumplist_target`
(`editor.rs:6393`) is a three-way answer:

| Variant | Condition |
|---|---|
| `OtherDocument` | `entry.identity != *identity` |
| `Stale` | `entry.byte > snapshot.len_bytes()` |
| `Honour(byte)` | otherwise |

and the refusal test — `an_entry_the_shorter_document_no_longer_contains_is_refused`
(`src/tui/commands/editor.rs:14886`) — actually shrinks the document under the
entry, presses `space j` `Enter`, and asserts the cursor is unmoved
(`jumplist_bytes(&app) == vec![0, 8]` before the delete, then
`cursor == before` after the refused jump) and that "the reason names the entry as
stale".

**The leaf row is a working verb.** `space j` is
`PrefixAction::Effect(EditorPendingEffect::OpenJumplist)` (`editor.rs:863`), the
effect variant exists (`editor_effect.rs:128`), and it dispatches to
`open_editor_jumplist_picker` (`commands/editor.rs:3677`). The leaf-authority
guard now passes with **4** unavailable reasons where M005's record counted
**5** — the difference is exactly `j` — and reports "6 leaf table(s), 52 leaf
glyph(s), **15** prefix binding(s), 4 unavailable leaf reason(s), no duplicate and
no unreachable leaf" (measured, exit 0).

**D1 resolved to (c), and the consequence is documented rather than implied.**
`docs/tui.md`'s jumplist section carries the paragraph: "**`Ctrl-i` and `Ctrl-o`
are not bound to this, and that is a decision rather than an omission** … This TUI
never pushes the terminal's keyboard-enhancement flags, so `Ctrl-i` is the byte
`0x09` and always arrives as `Tab`, which is already `ToggleComposerMode`;
`Ctrl-o` is already `OpenWorkspaceDashboard`, with `Shift+W` as its other
spelling. An arm for either would compile and never fire … The picker is the whole
delivered gesture, and the stepping pair is deferred with that reason on the
record." The key-authority guard agrees that nothing was shadowed: "2 guard-block
chord(s), **92** Normal-mode chord(s) … **8** Insert-mode chord(s), no shadowed
arm" (measured, exit 0). M005's record recorded 90 Normal-mode chords at its own
close, so the figure has moved since — this record does not attribute the delta,
because later milestones in the same tree add chords.

**The bound is disclosed in the right place.** The picker title carries `n/32`
and, at the bound, says the oldest entry is dropped; the disclosure is in the
title rather than under the rows because "a short editor window clips a row off
the bottom of the box, and a disclosure nobody can see is not a disclosure"
(`docs/tui.md`). `the_jumplist_picker_discloses_the_bound_it_holds_at` is the
test.

Status is conditional because the plan asks D1 to be "resolved and written into
this plan and the roadmap", and neither the plan file nor the roadmap carries the
resolution — the decision is recorded in `docs/tui.md` and in the code, which is
where a user and a maintainer need it, but the plan's own handoff requirement is
not met as written (§10). The usual operational evidence — green `quick` on a
committed tree — is also outstanding.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A bounded store of `(document identity, byte offset)` | `MAX_JUMPLIST_ENTRIES = 32` (`document_session.rs:722`); `JumplistEntry { identity, byte }`; push prunes at the cap | pass | Positions, never text. |
| Reaching the bound is disclosed, not silently dropped | push prunes and `:820` exposes fullness; `the_jumplist_picker_discloses_the_bound_it_holds_at`; the picker title carries `n/32` | pass | |
| Clearance with the rest of the presentation | `presentation.jumplist.clear()` (`editor.rs:7249`); `the_jumplist_does_not_survive_open_reload_resync_or_close`; `clearing_the_jumplist_takes_the_picker_with_it` | pass | The plan's open/reload/resync/reconnect/close list is exercised through the same transition the undo history uses. |
| Order is chronological, not positional, and must not be "normalised" | `the_jumplist_keeps_the_order_it_was_given_and_the_picker_reads_it_backwards`; the store's doc comment says so | pass | The plan's §16 warning, asserted. |
| A named destination records where the jump **started**; a motion records nothing | `record_jumplist_jump` takes `before` and returns `None` when the cursor did not move; `is_named_destination` admits only `BufferStart`/`BufferEnd` and names `j`/`w`/`Ctrl-d`/`gs`/`$` as steps | pass | |
| `gg`, `G`, `:goto`, `gd`, diagnostic stepping, symbol-picker selection each record one entry | `a_motion_records_nothing_and_each_named_jump_records_where_it_started` (`commands/editor.rs:15050`) asserts `gg` and `G` each record exactly one entry and that a no-op jump records none; `a_diagnostic_step_and_a_search_adoption_record_where_they_started` asserts the diagnostic step and search adoption | pass for `gg`/`G`/diagnostic/search | `:goto`, `gd` and symbol-picker selection are asserted by the `note_jumplist_jump` call sites, not per-site by a test. §10. |
| `j` and `w` record none | `a_motion_records_nothing_and_each_named_jump_records_where_it_started` asserts `j` leaves the store empty; `w` is excluded by `is_named_destination` and by the absence of a `note_jumplist_jump` call at the motion arms | pass for `j`; `w` by construction | |
| `space j` opens the picker through the real dispatch path | `editor.rs:863` → `editor_effect.rs:128` → `commands/editor.rs:3677`; `space_j_opens_the_jumplist_and_enter_returns_to_the_selected_position` | pass (test present) | |
| Selecting an entry returns the cursor to the recorded position | Same test; `select_jumplist_entry` (`commands/editor.rs`) | pass (test present) | |
| The picker lists newest first with enough context to tell entries apart | `render_jumplist_picker` (`components/editor.rs`); `the_jumplist_picker_lists_positions_newest_first`; `docs/tui.md`: "A row is the line the jump started on and that line's own text" | pass | Line number **and** the line's text — the stronger of the two the plan allowed. |
| The highlight does not wrap | `the_jumplist_highlight_stops_at_both_ends`; `move_cursor` clamps | pass | |
| An entry past the end of the document is **refused** with a notice, and the cursor does not move | `jumplist_target` → `JumplistTarget::Stale` (`editor.rs:6393-6404`); `an_entry_the_shorter_document_no_longer_contains_is_refused` (`commands/editor.rs:14886-14915`) shrinks the document and asserts `cursor == before` and a notice containing "stale" | pass | The plan's "refuse, do not clamp", the same rule M005 applied to comment toggling. |
| An entry from a different document is refused for the same reason | `JumplistTarget::OtherDocument` (`entry.identity != *identity`) | pass | |
| A refusal leaves the picker open so another row can be chosen | `docs/tui.md` states it; the picker's selection handler does not close on refusal | pass | Not asserted by a named test in this pass. §10. |
| Refusing is distinct from clamping in the notice | the notice text at `commands/editor.rs:3919`: "jumplist: that position is gone — the document is shorter than it was, so the entry is stale" | pass | |
| `space j` with nothing recorded says so and names the gestures that fill it | `docs/tui.md`; the picker's empty state | pass | An empty picker would be "a key that appears to do nothing". |
| No dead chord: `Ctrl-i`/`Ctrl-o` are not bound | no editor-route override added; key authority: 92 Normal-mode chords, no shadowed arm (measured); `docs/tui.md` records the reason | pass | |
| D1 is resolved and recorded | `docs/tui.md` carries the resolution and its cost; the **plan and roadmap do not** | **partial** | §10. |
| The leaf-authority guard passes with one fewer `Unavailable` row | "4 unavailable leaf reason(s)" (measured; M005's record counted 5) and `j` is absent from the unavailable record | pass | |
| The doc-drift guard passes in both directions | "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound" (measured, exit 0) | pass | |
| The text-authority guard passes with no new allowlist entry | `check_tui_editor_text_authority.py` exit 0, one informational borrow note | pass | The store holds offsets, not text. |
| No migration, protocol variant, crate or new authority | no `Cargo.lock` change attributable; the store is `TuiDocumentPresentation` state | pass | The plan's §15 requirement. |
| `cargo test -p codegg --lib tui::editor` green | **discharged 2026-11-13** by `scripts/verify.sh full` | pass | §10; 17,377 workspace tests, 0 failures. |
| `cargo test --test tui_render` green | **discharged 2026-11-13** by `scripts/verify.sh full` | pass | §10; render corpus 1,237 passed, 0 failed. |
| `bash scripts/verify.sh quick` green | **discharged 2026-11-13** by `scripts/verify.sh full`, which calls `quick` | pass | §10. |

## 3. Production implementation evidence

- `src/tui/document_session.rs` — the jumplist store: `MAX_JUMPLIST_ENTRIES`
  (`:722`), `JumplistEntry`, push with pruning (`:789`), the fullness predicate
  (`:820`), row access and clamped `move_cursor`.
- `src/tui/editor.rs` — `jumplist_target` (`:6393`), `is_named_destination`
  (`:6410`-ish), `record_jumplist_jump` (`:6429`), `jumplist_identity`, the
  `SPACE_LEAVES` row (`:863`), and the clearing call (`:7249`).
- `src/tui/editor_effect.rs:128` — `OpenJumplist`.
- `src/tui/commands/editor.rs` — the effect dispatch (`:3677`),
  `note_jumplist_jump`, `open_editor_jumplist_picker`,
  `handle_jumplist_picker_key`, `select_jumplist_entry`, and the stale-refusal
  notice (`:3919`).
- `src/tui/components/editor.rs` — `render_jumplist_picker`, which draws the rows
  and the `n/32` title.

Nothing in the plan's "explicitly out of scope" list was implemented: there is no
durable jumplist, no cross-document jumplist (entries carry an identity precisely
so a foreign one is *refused*), no `Ctrl-i`/`Ctrl-o` binding, no fuzzy behaviour,
and no second jumplist for the terminal panel.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_prefix_state.py

grep -rn 'MAX_JUMPLIST_ENTRIES' src/tui
grep -rn 'OpenJumplist' src/tui
grep -n 'fn jumplist_target' -A 45 src/tui/editor.rs
grep -rn 'fn .*jumplist[a-z_]*(' src/tui --include=*.rs
sed -n '2188,2260p' docs/tui.md
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_leaf_authority.py` | **pass** — "6 leaf table(s), 52 leaf glyph(s), 15 prefix binding(s), **4 unavailable leaf reason(s)**, no duplicate and no unreachable leaf", exit 0 |
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 92 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm", exit 0 |
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path", exit 0, with one informational `borrowed-document-field` note |
| `check_tui_editor_doc_drift.py` | **pass** — "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound", exit 0 |
| `check_tui_editor_prefix_state.py` | **pass** — "one pending-prefix field (pending_prefix), typed as EditorPrefix, no legacy mechanism left", exit 0 |
| `MAX_JUMPLIST_ENTRIES` | `= 32` at `document_session.rs:722` (measured) |
| `OpenJumplist` | 3 sites: leaf row, effect variant, dispatch (measured) |
| Jumplist tests | 7 test names containing `jumplist` (store order, clearing, picker order, picker bound, `space j` + `Enter`, highlight clamp, lifecycle) plus `an_entry_the_shorter_document_no_longer_contains_is_refused` (measured by name; none run here) |

### Not run

| Command | Why |
|---|---|
| `cargo test -p codegg --lib tui::editor` | Not run at the time of writing: compiling was expensive and three sessions were compiling, by instruction. **Discharged 2026-11-13** by the `scripts/verify.sh full` workspace sweep (17,377 tests, 0 failures). |
| `cargo test --test tui_render` | Same. **Discharged 2026-11-13**: render corpus 1,237 passed, 0 failed. |
| `cargo fmt --check`, clippy, `scripts/verify.sh quick` | Out of scope and forbidden for that session. **Discharged 2026-11-13** by `scripts/verify.sh full`, which runs `quick` → `run_guards()` (first line `cargo fmt --check --all`) and clippy `-D warnings`. |

## 5. Invariant review

- **Positions, never text.** Holds: the entry is `(identity, byte)`, and the
  text-authority guard passes with its rule that a field named `text`/`content`/
  `lines`/`body`/… or an owned `DocumentSnapshot` is a violation. The store is
  inside `TuiDocumentPresentation`, whose only `DocumentSnapshot` is a borrow.
- **Bounded and disclosed.** Holds — 32 with a title disclosure.
- **Cleared with the rest of the presentation.** Holds, and asserted.
- **An entry that cannot be honoured is refused, not approximated.** Holds, in a
  three-way typed answer plus a notice that names the entry as stale.
- **No dead chord.** Holds — the guard reports no shadowed arm and the two global
  owners of `Tab`/`Ctrl-o` are unchanged.
- **No second text buffer, no mutation outside `apply_local`.** Holds: a jump is
  a cursor move, and the milestone adds no mutation path at all.
- **The document and the code agree in both directions.** Holds for the leaf
  tables and the chord inventory (doc-drift guard, exit 0). The one place the
  documents lag is the plan/roadmap's D1 record (§10).

## 6. Failure and recovery review

- **Partial failure.** A picker selection whose entry cannot be honoured produces
  a notice and does not move the cursor (asserted at `commands/editor.rs:14883`).
- **Duplicate delivery / cancellation races.** None; pushing and stepping are
  synchronous and there is no async work.
- **Restart.** The store is empty after a restart by construction — it is
  presentation state, not persisted. Recorded here because the plan's §10
  requires the record to say the store is in-memory by design rather than omit
  the section.
- **Stale generation.** The store is cleared by the same transitions that clear
  the undo history, so an entry cannot outlive the text it addresses; and even if
  one did, `jumplist_target` refuses it rather than landing near it.
- **Bound reached.** Prunes the oldest and discloses in the picker's title.
- **Reload shrank the document.** Refused with a notice naming the reason, never
  clamped.

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency migration. Two compatibility
surfaces, and the second is the one worth recording:

1. **The leaf.** `space j` changes from a statement ("this editor keeps no
   visited-position history") to a picker. Additive; no transitional state, and
   the doc-drift guard confirms the document and the tables agree.
2. **D1's chord question resolved to (c), so there is no compatibility surface at
   all** — under (a) or (b) the editor route would have taken `Tab` or `Ctrl-o`
   from the global map, which is a user-visible narrowing and would have required
   a fourth stage in the key-authority guard's model. None of that happened: the
   guard's model is unchanged ("2 / 92 / 8, no shadowed arm") and `docs/tui.md`
   states the deferral with the wire-level reason.

Removal criteria for a transitional state: none; there is none.

## 8. Security review

No change, as the plan requires: a jump is a cursor move, not a read and not a
write. The two bounded properties that matter are the entry cap and the refusal
path — an entry can never address text the current attachment does not contain,
because the identity check precedes the offset check. There is no authorization
surface, no path validation, no secret and no network behaviour in this
milestone.

## 9. Documentation and operations

- `docs/tui.md:2188-2250` — the jumplist section: what records and what does not,
  the key table, the 32 bound and why the disclosure is in the title, the refusal
  rule, the clearing rule, and the D1 paragraph naming `Ctrl-i`/`Ctrl-o` and their
  current owners. This is the operational documentation and it is complete.
- `docs/tui.md:2159` — the `space j` row in the leaf table.
- The `space ?` keymap box follows from the same edit; the doc-drift guard passes
  in both directions, which is the mechanical check that the box and the tables
  agree (52 leaf gestures, 100 chords documented).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | D1 resolved to (c) and is recorded in `docs/tui.md` and implemented in the code, but the plan's §7 D requires the resolution to be "written into this plan and the roadmap", and neither carries it. Both still read as an open recommendation. | A reader of the planning documents sees a decision point where the milestone has already decided, which invites a re-litigation that the code and docs have settled. | Add one line to the plan's D1 section ("Resolved: (c) …") and to the roadmap's M006 block. The parent owns those edits. |
| low | **Partly closed 2026-11-13 — the plan's "each named jump pushes exactly one entry; a plain motion pushes none" now has a named test, but not for every jump.** `a_motion_records_nothing_and_each_named_jump_records_where_it_started` (`commands/editor.rs:15050`) asserts `j` records nothing, `gg` records exactly one entry at its start, a no-op `gg` records none, and `G` records one; `a_diagnostic_step_and_a_search_adoption_record_where_they_started` covers the diagnostic step and search adoption. What is still not asserted per-site is `:goto`, `gd` and symbol-picker selection. | The plan's stated acceptance is met by a test for the motions and two named jumps, and by inspection for the rest. The residual is narrower than this record first stated: three jump sites, not all of them. | Add one test that walks `:goto`, `gd` and a symbol-picker selection and asserts the entry count, which is exactly the plan's wording. |
| low | "A refusal leaves the picker open so you can pick another row" is documented but no test is named for it. | A refused selection that closed the picker would be a small usability regression with no detector. | Add the assertion, or accept the documentation as the contract. |
| low | **Discharged 2026-11-13.** `scripts/verify.sh quick`, `cargo fmt --check` and the two test lanes were not run by this record's author; the tree was uncommitted. | The plan's §11 command list had no result in this record. | None: the 2026-11-13 `scripts/verify.sh full` ran the workspace sweep (17,377 tests, 0 failures), the render corpus (1,237, 0 failures), `quick` → `run_guards()` and clippy `-D warnings`, all green on this tree. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The capability, the bound, the disclosure, the refusal rule, the clearing rule and
D1(c) are all delivered and documented. The operational condition is
**discharged**: `scripts/verify.sh full` ran on this tree at 2026-11-13 — 17,377
workspace tests and 8,441 feature-pass tests, 0 failures, `cargo fmt --check
--all` and clippy `-D warnings` clean, render corpus 1,237 passed — and `full`
calls `quick`, which calls `run_guards()`. What remains are the two evidence
items above: the picker-stays-open assertion, which no test names, and the
per-jump-count test, which now exists for `j`, `gg`, `G`, the diagnostic step
and search adoption but still not per-site for `:goto`, `gd` or symbol-picker
selection. A follow-up may add both without a corrective plan because they
assert behaviour the code already has. The plan/roadmap D1 line is a
documentation edit rather than a milestone defect.

What M007 and M008 may rely on unchanged: `space j`'s leaf row, the unavailable
set now holding 4 reasons, the prefix count at 15, and a jumplist that records
positions only.

## 12. Registry updates

- `plans/registry.md` line 30 (subsystem row): "M001–M005 implemented, M006 in
  flight, M007–M009 pending" → M006 conditionally closed with this record's path.
- `plans/registry.md` line 221 (the M006 milestone row): status "**ready**" →
  conditionally closed, with this record and the D1(c) resolution named.
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §12 row
  `M006 jumplist`: `ready for handoff` → `conditionally closed`, closure record
  `plans/closure/tui-editor-helix-capability-parity/006-status.md`, blocker
  "the per-jump assertions for `:goto`/`gd`/symbol-picker and the
  picker-stays-open assertion" (the `quick`/`fmt --check` half was discharged
  2026-11-13).
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §7 M006 body:
  record that D1 resolved to (c).
- `plans/implementation/tui-editor-helix-capability-parity/006-jumplist.md`
  header: `Status: ready for handoff` → `Status: conditionally closed`. **This
  record's author made this header edit**; the D1 resolution line is left for the
  parent because it is plan content rather than a status field.

## Correction log (2026-11-13)

- **§2, the three acceptance rows** — "`cargo test -p codegg --lib tui::editor`
  green | not run", "`cargo test --test tui_render` green | not run" and
  "`bash scripts/verify.sh quick` green | not run" → discharged by the
  2026-11-13 `scripts/verify.sh full` run: 17,377 workspace tests and 8,441
  feature-pass tests at 0 failures, render corpus 1,237 passed, and `full` calls
  `quick`.
- **§4, `Not run`** — the three rows (both test lanes; `fmt --check`, clippy,
  `quick`) → each marked discharged by the same run, which includes
  `cargo fmt --check --all` and clippy `-D warnings`.
- **§10, low (`quick`/`fmt`/tests not run)** — "Run on the merged tree from the
  merging session" → discharged 2026-11-13; the run happened on this tree.
- **§2, the two push-point rows** — "pass by construction … Not asserted per-site
  by a test this record can name" and "Same caveat" → a test *can* be named:
  `a_motion_records_nothing_and_each_named_jump_records_where_it_started`
  (`commands/editor.rs:15050`) asserts `j` records nothing and `gg`/`G` record
  exactly one entry each, and
  `a_diagnostic_step_and_a_search_adoption_record_where_they_started` covers the
  diagnostic step and search adoption. Superseding evidence: reading both test
  bodies in this tree. `:goto`, `gd` and symbol-picker selection remain
  unasserted per-site.
- **§10, low (per-jump-count test)** — "this record cannot name a test that
  asserts '`gd` pushes exactly one entry and `j` pushes none'" → partly closed:
  the test exists for `j`, `gg`, `G`, the diagnostic step and search adoption;
  the residual is `:goto`, `gd` and symbol-picker selection.
- **§11** — "the two evidence items above (a per-jump-count test and the
  picker-stays-open assertion)" → restated: the picker-stays-open assertion is
  still unasserted, and the per-jump-count test exists for four of the seven
  named jumps.
- **§12, the roadmap blockers cell** — "green `quick`/`fmt --check` on the
  merged tree; the per-jump-count test" → the gate half is discharged 2026-11-13,
  and the count test exists for four of the seven named jumps; the blocker that
  remains is the `:goto`/`gd`/symbol-picker per-site assertions plus the
  picker-stays-open assertion.
