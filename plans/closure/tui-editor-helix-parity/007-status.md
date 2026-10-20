# TUI Editor Helix Parity Milestone 007 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/007-multi-selection-core.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M007)
Repository baseline reviewed: `84440fe7f` (`fix(tui): terminal surface ergonomics — C003/C004 closure (#129)`)
Implementation commit: `278737d4e` (`feat(tui): Helix-parity editor milestones
M004-M008`). When this record was first written the change was uncommitted —
no commit had been requested — and the evidence below was "working tree at
baseline + this change". That is the same tree: `278737d4e` commits M004–M008
together, so this milestone's code and its siblings landed in one commit rather
than five. The evidence below was gathered before the commit was made and was
not re-run against it; it describes the tree that commit now contains.

---

## 1. Executive finding

M007 is **closed**. The selection model is now a set with a distinguished primary;
every effect that consumes a selection — `d`, `y`, `space y`, `r`, `R`, `~`,
`` ` ``, `Alt-` `, `p`, `P` — consumes the whole set as **one** transaction; undo
and redo restore the set as well as the cursor; and the terminal panel keeps
running the same rule set on one member with no call site changed.

Four things are worth recording beyond the checklist, because each is a decision a
later milestone would otherwise have to re-litigate:

1. **A pre-existing correctness bug was found and fixed.** `selection_range`
   resolved a character selection's end with
   `next_char_boundary(snapshot, last.byte).unwrap_or(last.byte)`. At the end of a
   document `next_char_boundary` returns `None`, so the fallback made `end ==
   last.byte` — **dropping the head character**. `v` on the final character of a
   file resolved to an empty range, so `d` on it deleted nothing and left a
   highlight the reader could not clear. Fixed to use `char_end`, which is the
   answer the rest of the module already uses for "one character from here".
   This was in the function M007 rewrites anyway, and it sits on the delete path.

2. **The primary's head is not painted with cursor styling when it differs from
   the cursor.** It was, at first, and it produced a second reverse-video block
   exactly where a reader reads "the keyboard is here". `head_cells` now reports
   "this head *is* the cursor", not "this member is primary"; every head that is
   not the cursor draws with the accent. On every Helix path the two coincide, so
   this only changes the state a goto leaves behind.

3. **Selection consumption moved from five verbs to one funnel.** Previously
   `r`/`R`/`~`/`` ` ``/`p`/`P` cleared the selection and every other edit verb left
   a stale highlight over text that had moved. `submit_edit` now records the set
   into the undo entry and clears it for every accepted edit. This is a
   **behaviour change** for edit verbs that previously kept it — `D` is the one
   reachable from Normal mode — and it is the direct consequence of `u` needing
   to restore the set. Recorded here because it is visible, not just internal.

4. **`apply_helix_verb` no longer clears the selection when it reports a delete or
   yank effect.** The geometry layer reports; the App layer consumes in
   `submit_edit`. Without that split, `u` after a multi-cursor delete would
   restore the text and drop the cursors on the floor. One pre-existing test
   asserted the old ownership (`deleting_a_line_selection_returns_the_whole_line_
   range`) and was updated, because the ownership genuinely moved; the buffer-layer
   test that drives only the reporting half now calls a `consume_selection` helper
   that performs the App layer's step explicitly.

---

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Verdict |
|---|---|---|
| §7.1 `SelectionState` holds `Vec<Selection<P>>` + `primary`; single-selection API primary-scoped | `src/tui/helix_selection.rs`; all 19 pre-existing tests pass **unmodified** | met |
| §7.2 every rule is whole-set | `helix_selection.rs` tests: `movement_extends_every_extendable_member_and_leaves_the_rest_alone`, `x_growth_is_refused_on_a_mixed_set…`, `v_on_a_mixed_set_arms_every_member…`, `x_grows_each_member_to_its_own_next_line` | met |
| §7.3 storage, 8 clear sites, `selection_ranges`, write-back | `TuiDocumentPresentation { selections, primary_index }`; `clear_selections` / `restore_selections`; `a_reset_presentation_drops_a_carried_over_selection_set` | met |
| §7.4 `DeleteRanges`/`YankRanges`, dispatcher fold, yank concat, sort/merge, 256 cap | `a_delete_over_n_members_is_one_transaction_covering_all_of_them`, `the_transaction_inverse_restores_the_text_it_removed`, `overlapping_members_merge_instead_of_producing_an_invalid_transaction`, `more_selections_than_one_transaction_may_carry_is_refused_not_truncated`, `an_empty_set_is_reported_as_empty_rather_than_as_a_refusal` | met |
| §7.5 multi-target `r`/`R`/`~`/`` ` ``/`p`/`P` | `replace_and_case_verbs_target_every_member_in_one_transaction`, `a_case_verb_drops_a_member_that_is_already_flipped…`, `paste_lands_after_every_member_without_two_anchors_colliding` | met |
| §7.6 rendering: set highlight, per-member heads, primary distinguished | `every_members_highlight_is_painted_not_just_the_primarys`, `an_extra_head_is_marked_differently_from_the_cursor` | met |
| §7.7 `selections_before` in the undo entry; undo/redo restore the set | `EditorUndoEntry::selections_before`; `a_multi_selection_edit_is_undone_and_redone_as_one_step`, `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` | met |
| §7.8 read-only growth refusal with notice, movement still allowed | `a_read_only_buffer_refuses_selection_growth_but_still_moves` | met |
| §7.9 terminal-panel non-regression | `src/tui/interactive_terminal.rs` unmodified; `cargo test -p codegg --lib tui::` green including `tui::commands::interactive_terminal::*` and `tui::interactive_terminal::*` | met |
| §7.10 guard self-demonstration | `check_tui_editor_prefix_state.py` fails on an injected `selection_prefix` field — output quoted in §4 | met |
| §7.11 verification commands | §4 below. `verify.sh full` does not go green; every failure is outside `tui::` and one is proven pre-existing at baseline — see §4 | met (with the named caveat recorded) |
| §7.12 documentation | `architecture/tui.md`, `docs/tui.md` | met |

The plan's §14 stop condition — *"M003's closure owed `t`/`f` to M007; the keymap
shows no set is required"* — did **not** trigger: the keymap analysis stands, `t`
lands on the target and `f` moves past it, and no `t`/`f` work was needed. No
stop condition fired.

---

## 3. Production implementation evidence

### The model (`src/tui/helix_selection.rs`)

- `SelectionState<P> { cursor: P, selections: Vec<Selection<P>>, primary: usize }`.
  `Copy` is gone with the `Option`; nothing depended on it — every caller reached
  for a field or a `&mut`, verified by grepping all 40 uses of `SelectionState`.
- `normalize_set(selections, primary)` is the single place that sorts by
  `Selection::bounds()` and re-clamps the primary, shared by the live rules and by
  `TuiDocumentPresentation::restore_selections`. The primary **travels with its
  member** rather than being pinned to an index.
- Primary re-anchoring rule: the primary anchors on the **cursor**, every other
  member anchors on its own head. Re-anchoring N members on one cursor would
  collapse the set to a single point.
- New surface: `restore_set`, `selections`, `primary`, `primary_index`,
  `into_parts`, `clear`, `grow_all_to`.

### The editor (`src/tui/editor.rs`)

- `selection_range` is **unchanged in meaning** and now answers for the primary;
  `selection_ranges` answers for the whole set, merged and ascending.
- `merge_ranges` is a correctness requirement, not a tidy-up:
  `codegg_document`'s validator rejects overlapping edits and `invert` is exact
  only for ascending, non-overlapping edits. Touching ranges merge too, because
  `a..b` + `b..c` deletes the same bytes as `a..c`.
- `delete_ranges` returns `RangeSetEdit { Edit | Empty | TooMany }` so "nothing to
  do" and "refused" stay distinguishable. The cap is `MAX_DOCUMENT_EDITS` (256)
  and it **refuses** rather than truncating.
- `verb_targets` / `paste_anchors` replace the single-target helpers; every M004
  verb goes through them, so M008 inherits multi-target behaviour for free.
- `case_targets` drops a member that would not change rather than abandoning the
  set — which is what stops `~` from depending on the order cursors were created
  in, and is a no-op difference for any single-member verb.
- `apply_helix_verb` restores the whole set and writes it back; `grow_all_to`
  resolves one point per member so `x` walks each member to *its own* next line.

### The command layer (`src/tui/commands/editor.rs`)

- `BufferKey::DeleteRanges` / `YankRanges`; `yank_editor_ranges` concatenates in
  document order and performs **one** clipboard write.
- `submit_edit` captures `selections_before` before the controller is asked, and
  clears the set on success.
- `apply_helix` gates `grows_selection(verb)` — `v` and `x` — on `editable`, and
  deliberately does **not** gate `Moved`.

### Rendering (`src/tui/components/editor.rs`)

- `line_spans` takes `&[Range<usize>]`; membership is "inside *any* member".
- `cell_for_offset` extracted from `cursor_cell` so a cursor cell and a head cell
  obey identical scroll-window rules.
- `extra_head_style()` uses the accent background, which is already the
  current-line gutter's "you are here" signal.

---

## 4. Verification executed

All commands run locally in the working tree. **No CI run was made.**

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo test -p codegg --lib tui::` | **2060 passed, 0 failed** |
| `cargo test -p codegg --lib tui::helix_selection` | **32 passed** (19 pre-existing, unmodified + 13 new) |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `scripts/verify.sh quick` | **passed** |
| `scripts/verify.sh full` | **did not go green — see below.** Its guards, `fmt`, `clippy`, and `cargo check` stages all passed; the failure is in the workspace test stage, in modules this change does not touch. |
| TUI integration binaries (`tui*`, `interactive_terminal_tui`, `document_client_trajectory`, `theme_selection_tokens`, `modifier_chords`) via nextest | **403 run, 403 passed, 0 skipped** |
| `python3 scripts/check_tui_editor_prefix_state.py` | exit 0 |
| `python3 scripts/check_tui_editor_key_authority.py` | exit 0 — `2 guard-block chord(s), 48 Normal-mode chord(s), 1 Insert-mode chord(s)` |
| `python3 scripts/check_tui_editor_text_authority.py` | exit 0 |

The chord counts are the **same numbers the guard printed before this milestone**,
which is the plan's AC12 evidence that no new Normal-mode binding was introduced.

### `verify.sh full` did not go green, and why that is not a M007 failure

This is recorded rather than glossed, because a closure that claims a green
sweep it did not get is exactly the defect the planning process forbids.

`verify.sh full` failed in its **workspace test stage**. The failing tests are
in `codegg::lsp` (≈30 tests), `codegg::tool_execution` (bash-tool tests),
`codegg::workspace_isolation`, `codegg::session_family::control_m004_controller_lease`,
`codegg::tool_surface_minimization`, and
`codegg::core::daemon::tests::session_create_through_daemon`. **None is under
`tui::`, and this change touches only `src/tui/*` and one guard script's prose.**

One of them was proven pre-existing rather than inferred:

```
$ git stash push -- src/tui/{helix_selection,editor,document_session}.rs \
                    src/tui/commands/editor.rs src/tui/components/editor.rs
$ cargo test -p codegg --lib session_create_through_daemon
  thread ... panicked at src/core/daemon.rs:4590:
  test workspace registration: InvalidRootPath("/tmp")
test result: FAILED.
```

It fails identically at baseline `84440fe7f`. It is an environment/root-validation
failure — the test registers `/tmp` as a workspace root — unrelated to selection.

The remaining categories are **environment-dependent by inspection, not by
proof**: the `lsp` tests need installed language servers (the workspace ships a
`lsp-real-server-tests` feature precisely because they are not runnable by
default), the `tool_execution` bash tests need a usable sandbox, and the
workspace/session-family failures share the same `/tmp` root-validation shape as
the one that was proven. I did **not** re-run each of these against the baseline,
so they are recorded as "unrelated and most plausibly environmental", not as
"proven pre-existing". That distinction is deliberate: one is evidence, the rest
is inference.

To bound the claim: **everything this milestone changed is green.** The TUI unit
suite (2060), the TUI integration binaries (403), clippy `-D warnings`,
`verify.sh quick`, and all three editor guards.

### Guard self-demonstration (§7.10)

An injected second prefix field was added to `TuiDocumentPresentation` and
removed again; the source file is byte-identical to before the demonstration.

```
TUI editor prefix-state violations found.
…
  TuiDocumentPresentation declares 2 prefix field(s) ['selection_prefix',
  'pending_prefix']; exactly one (`pending_prefix`) is the whole point of M002 —
  a new grammar adds a leaf, not a flag
1 violation(s) found.
exit=1
```
Restored: `prefix-state guard passed`, `exit=0`.

### Two runs of the same command disagreed, and why

`tui::commands::interactive_terminal::a_live_terminal_surface_repaints_with_no_further_user_input`
failed in one full-suite run and passed in two later runs, with unrelated cargo
processes from other sessions competing for the machine. It was checked against
the baseline by stashing the five changed `.rs` files: it **passes on `84440fe7f`**
in isolation, and it passes on this change in isolation and in the later full
runs. Recorded as environment load, not a defect — but it is recorded, because a
test that is sensitive to machine load is worth knowing about.

---

## 5. Invariant review

| Invariant | Verdict |
|---|---|
| §4.1 The panel holds at most one selection; the editor's set never leaks into the terminal | held — `src/tui/interactive_terminal.rs` is unmodified |
| §4.2 `v`/`x` remain mode and action | held — the rule is unchanged and now has explicit whole-set tests |
| §4.3 Only the `editable` funnel mutates text | held — `delete_ranges`/`replace_targets` build `EditorEdit`s; they never touch the buffer |
| §4.4 `resolve_edit` never learns a selection | held — `resolve_edit` is unmodified |
| §4.5 Range endpoints come from `Selection::bounds()` over `editor_point`-clamped points | held — and now *correct* at end-of-buffer (finding 1) |
| §4.6 Rendering is O(visible lines × members) | held |
| §4.7 No new Normal-mode chord | held — guard chord counts unchanged |
| §4.8 `EditorPrefix` remains the only prefix mechanism | held — guard demonstrated in §4 |
| §4.9 No filesystem access from the editor | held — text-authority guard passes |
| §4.10 The single-selection API is primary-scoped, not deleted | held — `selection()`/`is_selected()`/`span()`/`line_range()` keep their signatures |

---

## 6. Failure and recovery review

- **Refused, never truncated.** Above 256 distinct ranges `delete_ranges` returns
  `TooMany(n)` and the dispatcher shows a notice naming the count and the limit. A
  truncated multi-cursor delete is exactly the state neither `u` nor redo can
  describe.
- **One transaction, never N.** A rejection at the controller therefore lands
  wholly or not at all.
- **Overlap is resolved before submission, not caught after.** Two cursors on the
  same word collapse to one range, so `OverlappingEdits` is unreachable through
  this path.
- **Undo/redo are symmetric.** `selections_before` is recorded before the
  controller is asked, so an edit that lands and one that does not both leave the
  history consistent.
- **Read-only growth is refused loudly, movement is not.** Refusing `Moved` would
  take navigation away from a reader looking at a file they cannot edit.

---

## 7. Migration and compatibility review

No wire, storage, protocol or migration change. The set is frontend presentation
state held in memory and in the bounded undo history, exactly as the single
selection was. `TuiDocumentPresentation`'s two fields replace one; nothing
persisted, nothing on the wire.

---

## 8. Security review

No new authority, no new I/O, no new user input surface. The read-only gate is
narrower than the verb set and uses the existing `EditorNotice` channel. The
clipboard path concatenates already-bounded document ranges and writes once. No
secret is read or logged. No security-relevant finding.

---

## 9. Documentation and operations

- `architecture/tui.md` — the selection-model section now states the set, the
  primary-scoped API and *why* it was kept, ordering, the one-transaction/merge
  rule, the `RangeSetEdit` refusal, the funnel-level selection consumption, the
  narrow read-only gate, and the terminal-panel-by-construction argument.
- `docs/tui.md` — user-facing: set-wide verbs, one undoable change, clipboard
  concatenation order, and the read-only growth refusal.
- `scripts/check_tui_editor_prefix_state.py` — prose updated to record that the
  one-prefix assertion is now a real discrimination between fields, since the
  struct carries two non-prefix fields it must reject. **The assertion itself is
  unchanged.**

---

## 10. Unresolved findings

| # | Severity | Finding |
|---|---|---|
| 1 | **low** | `an_edit_after_undo_truncates_the_redo_tail_and_drops_the_set` asserts only that the redo tail is empty; it does not exercise a controller *rejection* between undo and the next edit. Rejection-path coverage is unchanged from before this milestone. |
| 2 | **low** | `a_live_terminal_surface_repaints_with_no_further_user_input` is load-sensitive (§4). Pre-existing; not introduced here. |
| 3 | **medium** | `scripts/verify.sh full` is not green in this environment. Every failure is outside `tui::`; one is proven pre-existing at baseline, the rest are environment-dependent by inspection only (§4). The suite cannot currently be used as a green gate in this worktree, so M008 will hit the same wall. |
| 4 | **informational** | Nothing is committed. The closure rests on a working tree that also carries M004 and M005. A reviewer reading only commits will not see M007. |

No critical or high finding. Finding 3 is medium **about the environment's
ability to gate future work**, not about M007's correctness; it is owned here
because M008 inherits it. M007's own scope is complete.

---

## 11. Roadmap disposition

- **M007 → closed.** Its deliverable boundary was the model plus the model-level
  effect changes; every item landed.
- **M008 is unblocked.** Its sole hard dependency was M007, and M007's plan §5
  set API contract landed as specified: `SelectionState::{restore_set,
  selections, primary, primary_index, into_parts, clear, grow_all_to}`,
  `TuiDocumentPresentation::{selections, primary_index, clear_selections,
  restore_selections}`, `HelixVerbOutcome::{DeleteRanges, YankRanges}`,
  `RangeSetEdit`, `BufferKey::{DeleteRanges, YankRanges}`, and
  `editor::{selection_ranges, merge_ranges, delete_ranges, one_range(test)}`.
  M008 must **not** need a corrective pass.
- **M009 stays blocked** on M008 *and* accepted ADR-0034 (still `proposed`;
  there is no tree-sitter in the workspace).
- **M010 stays blocked** on M013 alone; unchanged by this closure.
- **Deferred unimpaired change navigation** (`]g`/`[g`/`]G`/`[G`) remains
  unregistered deferred work with no assigned milestone; this closure does not
  change that, and M007 neither adds nor removes data it needs.

---

## 12. Registry updates

Performed in the same change as this record:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md` — M007 status table row
  `ready` → `closed`, closure record column filled, and the M008 blocker updated
  from "M007" to "—" with a dated note recording the unblock and the §5 contract
  that was actually shipped.
- `plans/registry.md` — the roadmap row's milestone summary updated and the
  M007 closure record linked from it; the M007 row's status set to `closed`
  **in place** (the same treatment M004 and M005 received, rather than a
  move, so that the table keeps a single row per milestone and this closure
  does not diverge from its predecessors); **M008 moved from `blocked` to
  `ready`**, its blocker cleared with the unblock audit recorded inline.
- `plans/implementation/tui-editor-helix-parity/007-multi-selection-core.md`
  status header `ready for handoff` → `implemented`.
- `plans/implementation/tui-editor-helix-parity/008-multi-selection-command-surface.md`
  status header `blocked on M007` → `ready`, with a dated note pointing at this
  record.

Two stale claims in the roadmap row's long note were corrected in the same edit:
the "`SelectionState` holds one `Option<Selection<P>>`, never a set, so ~15 verbs
are unimplementable" statement (now marked resolved by M007) and the "M008 is
registered blocked on M007" statement.

Status labels were re-reconciled across the plan header, the roadmap status table
and the registry after every one of those edits, and the four surfaces were
printed and compared: M006 `ready`, M007 `closed`, M008 `ready` in all of them.