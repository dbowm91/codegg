# TUI Editor Helix Capability Parity Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/001-selection-shape-verbs.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-001--selection-shape-verbs`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same working tree also carries
  M002–M009 of this roadmap and the whole
  `tui-modularity-and-render-invariants` line. No statement below is
  attributable to a SHA.

## 1. Executive finding

The capability is delivered. Seven Helix selection-shape verbs — `X`, `Alt-x`,
`_`, `J`, `Alt-J`, `>` and `<` — are bound in Normal mode, each with exactly one
owner, each reaching the document through the existing funnels
(`apply_helix` for the three that write nothing, `apply_helix_edit` for the four
that write), and each proved reachable through the real dispatch path rather
than through a helper.

The fallthrough this milestone exists to close is closed: those keys previously
had no owner in `handle_normal_key`, so they fell through `BufferKey::Unhandled`
to the composer and typed themselves into the prompt box. They are now claimed
by the buffer.

Two decisions the plan required be recorded rather than assumed are recorded in
the code where the next reader will be: the exact line-bounds boundary
(`X` takes the line **including** its terminator, which is what makes `d` on a
line selection remove the line rather than leave a blank one), and the
indent-unit source (D1: detected from the document, bounded prefix, smallest
positive leading-space run, tab-majority wins, four-space fallback disclosed
through the notice).

The status was conditional for two reasons, neither of them the capability, and
the gate half is now discharged: `scripts/verify.sh quick` and `cargo fmt
--check` were not run by this record's author, and the plan's §4 invariant "the
comment ratchet holds" did not hold — `check_comment_budget.py` was red, with
three of its 20 violations on this milestone's edit path (§10). The 2026-11-13
`scripts/verify.sh full` run passed on this tree — 17,377 workspace tests and
8,441 feature-pass tests, 0 failures, `cargo fmt --check --all` clean, clippy
`-D warnings` clean — and `full` calls `quick`, which calls `run_guards()`, so
both gate conditions are met and the three editor-path ceilings are re-banked in
`docs/comment-budget.toml` (§10). What remains is one **open owner question**,
not a gate: whether the `space ?` box's omission of `_` and `Alt-d` is
non-gating (§10, §11).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `X` widens every member to the lines it touches | `HelixVerb::ExtendToLineBounds`; `src/tui/editor.rs` test `x_widens_an_inner_selection_to_the_line_and_a_second_press_changes_nothing` asserts `4..8` for a selection on line 1 of `"one\ntwo\n"` | pass | Range includes the terminating newline, so `d` removes the whole line. |
| `X` a second time is a no-op that reports nothing | same test: second `apply_helix_verb` returns `Handled` with `selections` unchanged | pass | Not an error, as the plan requires. |
| `Alt-x` narrows, and recovers the selection `X` widened where that is recoverable | `alt_x_recovers_the_line_bounds_selection_and_narrows_a_narrower_one`: `0..12` → `0..11` exactly; a selection narrower than the line mapping to `0..11` | pass | Both branches asserted, and the difference between them is the assertion. |
| `_` trims leading and trailing whitespace | `trim_strips_both_edges_and_keeps_an_all_whitespace_member_in_the_set`: padded member `0..=5` → `2..4` | pass | Trimming writes nothing; the document bytes are unchanged. |
| An all-whitespace member collapses to an empty selection at its own start and stays in the set | same test: two members before and after, second collapses to `(7, 7)`, `primary_index` still `0` | pass | The plan's rule that a vanishing member would silently move the primary. |
| `J` joins with one space and the following line's indentation does not survive | `joining_two_lines_replaces_the_terminator_and_indent_with_one_space`; dispatch test `capital_j_joins_the_covered_lines_through_the_dispatch_path` asserts `"one\n    two\n"` → `"one two\n"` and one newline remaining | pass | |
| `Alt-J` leaves the inserted space selected | `alt_j_leaves_the_inserted_space_selected_through_the_dispatch_path`: holds `3..4` and the cursor is at `3` | pass | `JoinLines { select_space }` is one variant with a flag, justified in the enum doc. |
| A multi-member join is one undo step | `capital_j_...` asserts `undo_depth == 1` | pass | |
| `>` inserts one indent unit on every covered line and makes the selection line-wise first | `greater_than_indents_every_covered_line_through_the_dispatch_path`: `"a\n  b\n"` → `"  a\n    b\n"`, selection afterwards `0..10` (line-wise over both), `undo_depth == 1` | pass | Two-space unit detected from the document. |
| `<` removes exactly one unit and does not eat content when a line is indented less than one unit | `outdent_removes_at_most_one_unit_and_stops_at_non_whitespace`: a padded line loses two spaces, an unindented line produces no edit at all, a tab-led line loses one tab; `less_than_removes_one_indent_unit_through_the_dispatch_path` | pass | The unindented line's absence from the edit list is the "does not eat content" assertion. |
| The indent unit comes from the document, with the fallback disclosed | `detect_indent_unit` (`src/tui/editor.rs`), its D1 comment, and `the_indent_unit_is_detected_from_the_document_and_defaults_to_four_spaces` over two-space, four-space, tab, tab-majority mixed, space-majority mixed and unindented inputs | pass | Each case asserts the `(unit, detected)` pair, so "defaulted" is distinguishable from "detected". |
| Every one of the seven keys is reachable through the real dispatch path | Seven tests in `src/tui/commands/editor.rs` under the `M001: line-shape verbs, through the dispatch path` heading: `capital_x_…`, `alt_x_…`, `underscore_…`, `capital_j_…`, `alt_j_…`, `greater_than_…`, `less_than_…` | pass | Each presses the key; none calls the resolver. |
| `X`, `J`, `Alt-J`, `>`, `<` and `_` are refused on a read-only attachment with a visible notice, document byte-identical | `every_shape_verb_that_grows_or_writes_is_refused_when_read_only` iterates exactly those six, asserting the key is still claimed, `notice` contains `Read-only`, bytes unchanged and no history recorded | pass | |
| `Alt-x` remains available on a read-only attachment | `alt_x_narrows_a_selection_on_a_read_only_attachment` | pass | The plan's narrow reading: requested growth is refused, navigation-like narrowing is not. |
| The key-authority guard reports no shadowed arm | `scripts/check_tui_editor_key_authority.py`: 2 guard-block, 89 Normal-mode, 8 Insert-mode chords, no shadowed arm | pass | See §4 for why the count reads 89 rather than 81. |
| The doc-drift guard passes in both directions | `scripts/check_tui_editor_doc_drift.py`: 52 leaf gestures and 98 chords documented; every key declared unbound is unbound | pass | |
| `docs/tui.md` gains the seven keys and loses any "not bound" entry naming them | `git diff -- docs/tui.md` adds the `X`, `Alt-x`, `_`, `J`, `Alt-J` rows and the prose for each | pass | |
| No second text buffer, no new mutation path | `scripts/check_tui_editor_text_authority.py` passes with no allowlist edit | pass | One `note` line reports a borrowed `DocumentSnapshot` field at `src/tui/components/editor.rs:205`, which is a borrow, not a retained copy. |
| No migration, protocol variant, crate or new authority | No `codegg-core`, `codegg-document`, `codegg-protocol` change; `Cargo.toml` unchanged for this milestone | pass | |

## 3. Production implementation evidence

- `HelixVerb` (`src/tui/editor.rs`) gains `ExtendToLineBounds`,
  `ShrinkToLineBounds`, `TrimSelections`, `JoinLines { select_space }` and
  `IndentSelection { outdent }`. Each carries the boundary rule in its doc
  comment, including the two the plan forbade guessing at.
- `src/tui/commands/editor.rs` gains the seven arms. `X`, `Alt-x` and `_` call
  `apply_helix` (selection-only); `J`, `Alt-J`, `>` and `<` call
  `apply_helix_edit`, so undo, the read-only gate, the retained-bytes bound and
  the selection-clearing rule are inherited rather than re-implemented.
- Punctuation arms accept both `NONE` and `SHIFT` for `X`, `>`, `<`, `_` and
  `J`, following the existing rule that a terminal may or may not set `SHIFT`
  for a shifted glyph.
- `Alt-x` is claimed before any `x` arm could see it; `x` itself is `NONE`-only,
  so the two are distinct events rather than a shadowing pair.
- `Alt-_` (adjacent merge) is untouched; bare `_` is the trim verb.
- `outdent_bytes` removes at most one unit and stops at non-whitespace, which is
  what keeps `<` from eating content on a short-indented line.
- `detect_indent_unit` scans a bounded prefix (`MAX_RENDER_LINE_BYTES`, the
  frame's own per-line budget) so one keystroke never costs more than one line
  of rendering.
- `docs/tui.md` and the editor's `space ?` box (`EDITOR_KEYMAP` in
  `src/tui/components/editor.rs`) were updated in the same change as the arms,
  which is the plan's §16 requirement: updating a guard or a document after the
  fact is how the predecessor's dead-arm defect survived.

Nothing in the plan's "explicitly out of scope" list was implemented: no counts,
no syntax-node objects, no formatter interaction, no configuration key, no block
reflow.

## 4. Verification executed

### Commands run

All commands were run locally on the host named in this record, in
`/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`.

```bash
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_leaf_authority.py

# the lib test target, run from the already-built test binary because three
# sibling sessions held the cargo build lock (see below)
./target/debug/deps/codegg-ef34c509a35311b3 tui::editor
./target/debug/deps/codegg-ef34c509a35311b3 tui::commands::editor

# the whole render family, same binary-built-not-cargo-run caveat
./target/debug/deps/tui_render-17e4c7be3ef479f9
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 89 Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm" |
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path"; one informational `note` for a borrowed-document-field at `src/tui/components/editor.rs:205` |
| `check_tui_editor_doc_drift.py` | **pass** — "52 leaf gestures and 98 chords are documented, and every key declared unbound is unbound" |
| `check_tui_editor_leaf_authority.py` | **pass** — "6 leaf table(s), 52 leaf glyph(s), 13 prefix binding(s), no duplicate and no unreachable leaf" |
| lib tests, filter `tui::editor` | **pass** — 292 passed, 0 failed, 7718 filtered out |
| lib tests, filter `tui::commands::editor` | **pass** — 259 passed, 0 failed, 7751 filtered out |
| `tests/tui_render` family, no filter | **pass** — 1217 passed, 0 failed, 0 filtered out, finished in 7.23s |
| `python3 scripts/check_comment_budget.py` | **fail at the time of writing; superseded 2026-11-13 — see the correction log.** 20 violations tree-wide, three of them on this milestone's edit path: `src/tui/editor.rs` 3320 comment lines against a ceiling of 2926, `src/tui/commands/editor.rs` 2481 against 2275 (ratio 0.2304 against 0.2270), `src/tui/components/editor.rs` 842 against 840. Those ceilings are now banked at 3693 / 3110 / 902 and the guard passed inside the 2026-11-13 `full` |

Two honesty notes on how these were produced:

- The two `cargo test -p codegg --lib` invocations and the whole
  `tests/tui_render` family were **not** run as `cargo` commands. Three sibling
  sessions were building in the same target directory, so every `cargo test`
  invocation queued behind the build lock. The compiled test binaries
  (`target/debug/deps/codegg-ef34c509a35311b3`, mtime 04:27:56;
  `target/debug/deps/tui_render-17e4c7be3ef479f9`, mtime 04:26:10) were executed
  directly with the same filters `cargo test` would pass. Both postdate the
  newest source file on this path (`src/tui/commands/editor.rs`, mtime
  04:11:16), so the results describe the tree as it stood at those times rather
  than an older one. The tree kept moving after that: other sessions
  changed `src/tui/components/section.rs`, `src/tui/components/dialogs/agent.rs`,
  `src/main.rs` and `crates/codegg-core/src/session/request_ledger.rs`, and at one
  point the library did not compile at all (`E0599`, resolved by its owner
  within minutes). Read these figures as the tree at those mtimes, not as a
  standing guarantee. `cargo test --test tui_render` and
  `-- invariants` were also run through `cargo` in this session; the `invariants`
  member's final run was 24 passed, 0 failed.
- The Normal-mode chord count is **89**, not the 81 that seven chords added to
  the plan's recorded 74 would give. The difference is later bindings from this
  same roadmap (M003 regex search, M005 comment toggling, M006 jumplist, M008
  recording), which are present in the same uncommitted tree. The per-milestone
  figure cannot be re-measured because there is no commit between the
  milestones; the plan's own §3 records 74, and the seven M001 arms are
  individually visible in `handle_normal_key`.

### Not run

| Command | Why |
|---|---|
| `cargo fmt --check`, `bash scripts/verify.sh quick` | Not run at the time of writing: `quick` runs the routine guards and a workspace check over a tree other sessions were editing, and a red result there would not have been attributable to this milestone. **Discharged 2026-11-13** — the `scripts/verify.sh full` run passed on this tree (`full` → `quick` → `run_guards()`, whose first line is `cargo fmt --check --all`). |

A cross-milestone observation, recorded because it affects how the two TUI
records in `plans/closure/tui-modularity-and-render-invariants/` should be read:
at 04:22 the `invariants` family was **red** (23 passed, 1 failed) on
`the_recorded_i5_baseline_is_exactly_what_the_harness_measures`, reporting 330
recorded contrast entries that no longer reproduced. That measurement was taken
against a tree in which `src/tui/components/dialogs/info.rs` carried an
in-flight change from another session; that change was reverted, and the same
command is **green** (24 passed, 0 failed) at 04:32. This milestone does not
touch the render path, so neither reading is evidence about it.

## 5. Invariant review

- **No second text buffer.** Every read is a scoped `try_snapshot()` borrow; no
  verb retains a `DocumentSnapshot` or a copy of the text. The text-authority
  guard passes with no allowlist edit.
- **One owner per chord.** The key-authority guard reports no shadowed arm.
  `Alt-x` cannot be shadowed by `x` because `x` is `NONE`-only.
- **One undo step per gesture.** `capital_j_…` asserts one entry for a
  multi-line join; `greater_than_…` asserts one for a two-line indent;
  `outdent_removes_at_most_one_unit_and_stops_at_non_whitespace` asserts one
  transaction for a three-line outdent.
- **Read-only refusal is visible.** Six writing/growing verbs each assert the
  notice, the unchanged bytes and an empty history; `Alt-x` asserts it stays
  available.
- **Positions, never text.** Every verb resolves against the live snapshot and
  returns byte ranges or a `TextTransaction`.
- **Bounded work per gesture.** `detect_indent_unit` scans at most
  `MAX_RENDER_LINE_BYTES`; the edit count is bounded by the existing
  `MAX_DOCUMENT_EDITS` rule.
- **The published keymap agrees with the code in both directions.** The
  doc-drift guard passes in both directions in the same change as the arms.

## 6. Failure and recovery review

- **Partial failure.** Each verb resolves once against one snapshot and submits
  one transaction. If the controller rejects it the notice reports the rejection
  and the selection is left as it was; the transaction is all-or-nothing, so no
  half-applied indent is possible.
- **Duplicate delivery.** Not applicable — synchronous local gestures with no
  request/response.
- **Restart.** No state survives a restart by construction: this milestone
  stores nothing. **Stated explicitly as the plan's §10 requires**, rather than
  omitted.
- **Cancellation races.** None; no async work is added and no request is issued.
- **Stale generations.** `try_snapshot()` cannot return a stale snapshot.
- **Bound reached.** An indent or join that would exceed `MAX_DOCUMENT_EDITS` is
  refused rather than truncated, matching `delete_ranges`' established rule. The
  M002 sibling of this assertion
  (`an_indent_past_the_transaction_limit_is_refused_and_changes_nothing`)
  exercises the same rule for `>`.

## 7. Migration and compatibility review

No storage migration, no protocol variant, no configuration key, no crate
change. The only compatibility surface is the **keymap**: seven keys that
previously reached the composer on the editor route now belong to the buffer.
That narrows the composer's keyspace on the editor route and nowhere else, which
is the documented and intended effect of adding a binding, and `docs/tui.md`
records it in the same change. Rollback is deleting the arms; there is no
persisted state to unwind.

## 8. Security review

No authorization surface changes. Every writing verb goes through
`apply_local` — the authorized mutation path — and the read-only gate is the
existing one (`grows_selection` / `mutates_buffer`), exercised rather than
assumed. No path, secret, process or network surface is touched. The verbs
cannot exceed the transaction budget, so a single keystroke cannot be turned
into unbounded work.

## 9. Documentation and operations

- `docs/tui.md` — the seven keys in the Normal-mode table, the boundary rules
  (`X` includes the terminator; `Alt-x` narrows to content bounds), the
  indent-unit detection rule, and the read-only refusal list.
- `src/tui/components/editor.rs` `EDITOR_KEYMAP` — the `space ?` box rows for
  `x / X / Alt-x`, `J / Alt-J` and `> / <`.
- `src/tui/editor.rs` — D1 recorded as a code comment on `detect_indent_unit`.
- Guards: `check_tui_editor_key_authority.py`, `check_tui_editor_leaf_authority.py`,
  `check_tui_editor_text_authority.py`, `check_tui_editor_doc_drift.py` all pass
  unchanged in logic, with figures moved in the same change as the chords.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | **Discharged 2026-11-13.** The comment corpus ratchet — the plan's §4 "the comment ratchet holds" — did **not** hold at this record's close: `check_comment_budget.py` reported 20 violations tree-wide, three on this milestone's edit path, with `src/tui/editor.rs` 394 comment lines over its recorded ceiling, `src/tui/commands/editor.rs` 206 over (and over its ratio ceiling), `src/tui/components/editor.rs` 2 over. The three ceilings are now banked in `docs/comment-budget.toml` (`editor.rs` 3693, `commands/editor.rs` 3110, `components/editor.rs` 902) and the guard passed inside the 2026-11-13 `scripts/verify.sh full`. | The verb comments are unusually dense (each boundary rule is explained where it applies). The ratchet exists so a split does not silently redistribute prose; a ceiling that is simply exceeded is the failure the ratchet is for, and re-banking is how it is paid for. | None. One tree-wide violation has since appeared in `src/tui/components/dialogs/keybind.rs` from the corrective plan's WP D deletion, outside this path and owned by that work package's manifest bank. |
| low | **Open owner question — the `space ?` keymap box omits two of the ten keys this pair of milestones bound: `_` (trim) and `Alt-d` (the no-yank delete).** `docs/tui.md` carries both. The box is `EDITOR_KEYMAP` (`src/tui/components/editor.rs:253`) and its rows still read `d / y` and `c / Alt-c`, so the omission is current, not historical. | A reader who learns the verbs from the in-editor box will not discover them; there is no guard on the box, only on `docs/tui.md`. The owner has **not** ruled on whether this documentation item is non-gating, so this record neither resolves it nor drops it. | Owner decision required: either **accept it as non-gating** — which would settle it with one sentence recorded here, on the ground that `docs/tui.md` is the complete document and the box is a condensed in-editor aid — or **require the two rows**, in which case the settlement is the two added `EDITOR_KEYMAP` rows plus a doc-drift run. Until one of those is recorded, the item stays open. |
| low | `TrimSelections` (`_`) is classified as `grows_selection` and so refused on a read-only attachment, although the verb only ever narrows. | Deliberate: one predictable answer for the shape family, and the plan's §13 refuses `_` explicitly. Recorded so a later reader does not read it as an oversight. | None. The code comment at `grows_selection` carries the reason. |
| low | The per-milestone Normal-mode chord figure cannot be re-derived; the guard reads 89 against a milestone that added 7 to 74. | Reviewers cannot confirm from a number that only M001's seven chords moved. | Land the roadmap as separate commits, or accept the arm-by-arm reading in `handle_normal_key`. |

## 11. Roadmap disposition

**Milestone closed (owner decision, 2026-11-14).** The capability, its tests, its guards, its
documentation and its two recorded decisions are complete and verified. The
conditions were:

- **Condition.** Three things had to hold on the merged tree before M002 of this
  roadmap was cited as depending on a closed M001:
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
not a gate: whether the `space ?` box's omission of `_` and `Alt-d` is
non-gating (§10). Its settlement is either an owner statement that the box is a
condensed aid and `docs/tui.md` is the complete document, or the two added
`EDITOR_KEYMAP` rows. This record does not decide it.

- **Risk.** Low on capability: this milestone adds no production render path and
  no runtime state. The whole `tests/tui_render` render family passes
  (1217/1217 at this record's close; 1,237 in the 2026-11-13 `full`), the four
  editor guards pass, and 551 filtered library tests pass. The former process
  risk — `quick` and the comment ratchet both red in a tree carrying several
  uncommitted milestones — is discharged. The remaining risk is that a key
  listed in `docs/tui.md` is not listed in the in-editor box.
- **Exact future evidence.** Satisfied for conditions 1–3 by the 2026-11-13
  `scripts/verify.sh full` run and the re-banked ceilings. Still owed: the owner
  ruling on the box, and the commit that carries this plan (WP 0).

## 12. Registry updates

- Mark roadmap M001 **conditionally closed** and link this record.
- Leave M002's own status to its record
  (`plans/closure/tui-editor-helix-capability-parity/002-status.md`); do not
  mark M003 ready on the strength of this record alone.
- Record in the roadmap that the guard's expected Normal-mode count is 89 at
  this tree state and that later milestones must move it, not rediscover it.

## Owner decision (2026-11-14)

**Decision 5 of the seven, taken as B: the box now names the two keys.**

The `space ?` box (`EDITOR_KEYMAP`, `src/tui/components/editor.rs:253`) read
`d / y` and carried no `_` row, while `docs/tui.md` documents `Alt-d` and `_`.
The omission was current, not historical: a reader who learns the verbs from the
in-editor box never discovered trim or the no-yank delete.

It now reads `d / Alt-d / y` — "delete into the unnamed register / delete without
yanking / yank it and the clipboard" — and carries `_ · trim the selection to
whole lines`. The list is scrollable, so no row was displaced. `cargo test -p
codegg --lib -- tui::editor tui::components::editor` is 392 passed.

The two remaining low findings are unaffected and stay recorded: `_` is
classified `grows_selection` and refused on a read-only attachment (deliberate,
one predictable answer for the shape family), and the per-milestone chord figure
cannot be re-derived.

## Correction log (2026-11-13)

- **§1, the status paragraph** — "`scripts/verify.sh quick` and `cargo fmt
  --check` were not run … `check_comment_budget.py` is red, and three of its 20
  violations are on this milestone's edit path" → the gate half is discharged by
  the 2026-11-13 `scripts/verify.sh full` (17,377 + 8,441 tests at 0 failures,
  formatting and clippy clean) and by the three re-banked ceilings; what remains
  is the owner question, not a gate.
- **§4, the comment-budget result row** — "**fail** — 20 violations …" → marked
  superseded: the three ceilings are banked at 3693 / 3110 / 902 and the guard
  passed inside the 2026-11-13 `full`.
- **§4, `Not run`** — "`cargo fmt --check`, `bash scripts/verify.sh quick` …
  Not run" → discharged by the 2026-11-13 `scripts/verify.sh full`
  (`full` → `quick` → `run_guards()`, whose first line is
  `cargo fmt --check --all`).
- **§10, low (comment ratchet)** — "the plan's §4 'the comment ratchet holds' —
  does **not** hold … 20 violations" → discharged 2026-11-13 by the bank and
  the `full` run.
- **§10, low (`space ?` box)** — "The `space ?` keymap box omits two of the ten
  keys … Add the two rows, or record … why the box is a condensed list" →
  restated as an **open owner question** with its two possible settlements,
  because the owner has not ruled on whether this documentation item is
  non-gating. The omission itself is verified current: `EDITOR_KEYMAP`
  (`src/tui/components/editor.rs:253`) carries `d / y` and `c / Alt-c`, not `_`
  or `Alt-d`.
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
- **Owner decision, 2026-11-14** — `Alt-d` and `_` added to the `space ?` box; the record is `closed`.
