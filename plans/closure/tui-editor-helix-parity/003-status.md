# TUI Editor Helix Parity — M003 closure: Character-finding motions

Status: **closed**

Repository baseline: `719cfeb5d` (branch `feat/auto-20261017-80a41db7`)

Implementation plan:
`plans/implementation/tui-editor-helix-parity/003-character-finding-motions.md`

Predecessor closure:
`plans/closure/tui-editor-helix-parity/002-status.md`

Source roadmap:
`plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M003)

---

## 1. Outcome

`f`, `F`, `t` and `T` are available in `NOR` and search the whole document
rather than the cursor's line. `Alt` + `.` repeats the last find and is cleared
by any other motion. The find target is armed through M002's `EditorPrefix`
grammar rather than a new flag, so it disarms at every point the existing
prefixes already disarm.

The scan is bounded at `MAX_EDITOR_FIND_BYTES` and **discloses** when it stops
short, distinguishing "that character is not that way" from "I stopped looking".
A find that lands says nothing, so an ordinary keystroke leaves the banner
row exactly as it did before this milestone.

---

## 2. What was built

### 2.1 The motion vocabulary (`src/tui/editor.rs`)

| Type | Role |
|---|---|
| `FindKind` | `Next` / `Previous` / `TillNext` / `TillPrevious`; carries `forward()` and `command(target)` |
| `FindMotion` | `{ kind, target }` — the repeatable-motion record |
| `FindOutcome` | `Found(offset)` / `Absent` / `Bounded` |
| `FindNote` | the disclosure rendered in the banner: `None` / `Absent(char)` / `Bounded(char)` |
| `MAX_EDITOR_FIND_BYTES` | `= MAX_EDITOR_SEARCH_BYTES` |
| `find_char` | the bounded cross-line scan |

Four `EditorCommand` variants carry the target: `FindNext(char)`,
`FindPrevious(char)`, `FindTillNext(char)`, `FindTillPrevious(char)`.
`EditorCommand::find_motion()` is the single place that answers "is this a find
motion", so the recorder and the dispatcher cannot disagree.

### 2.2 The prefix composition (`src/tui/commands/editor.rs`)

`EditorPrefix` gained `FindNext` / `FindPrevious` / `FindTillNext` /
`FindTillPrevious`. All four have `leaf() == None` — there is no glyph that
could complete them — and `find_kind()` identifies them. `resolve_pending_prefix`
resolves them **before** the leaf table, because their next key is data rather
than a command: reaching the leaf match would send a perfectly good target down
the mismatch path and out to the composer.

`find_target_char` is deliberately a separate function from `leaf()`. A key
that is a target never resolves against the keymap; a key that is a leaf never
becomes a character. Collapsing the two would have been the shortcut that
produced the whole class of defect this milestone guards against.

No new field was added to `TuiDocumentPresentation` for the target: it is
consumed within the one keystroke that arms it.

### 2.3 The repeat slot

`TuiDocumentPresentation` gained two fields, neither of which is a prefix field
(the M002 guard therefore still passes unchanged):

- `last_find: Option<FindMotion>` — written by `apply_motion` on **every**
  find, including one that found nothing, because Helix sets the repeat slot
  by running the command;
- `find_note: FindNote` — why the last find did not land, if it did not.

`forget_find()` clears both together; a disclosure about a find that cannot be
repeated, or a repeatable find whose disclosure is missing, are both a status
row lying about the last keystroke.

---

## 3. Decisions taken where the plan left room

### 3.1 `t`/`T` land on the target — and what that costs

The plan states five times that `t` and `T` leave the cursor **on** the target,
and names "landing before the target" as *the* non-vacuity hazard. Acceptance
criterion 2 says the same. Both were followed.

The consequence is worth stating plainly rather than burying: **at this
milestone `t`/`T` are cursor-equivalent to `f`/`F`.** They scan the same, in the
same direction, and land on the same byte. Nothing observable distinguishes
them today.

That is not an accident of implementation, it is a single-cursor consequence.
In Helix the head of a till-motion lands *past* the match so the selection
covers it; `find_till_unbound` returns `match_start + ch.len_utf8()`. With one
cursor and no interval, "the head is past the match so the match is inside the
selection" translates to "the cursor is on the match". The real distinction
between `f` and `t` is a **selection** property, and CodeGG does not have a
selection set until M007.

The four arms are therefore spelled out separately with the reason in the
`EditorCommand` doc comments rather than collapsed into two. Collapsing them
would have quietly adopted the vi/kakoune reading the plan rejects, and the
collapse would then be invisible to M007 when it goes looking for the
difference.

**This is recorded as a known gap, not resolved.** If a user reads `docs/tui.md`
and expects `t` to behave differently from `f`, they will be disappointed; the
doc says `as f` rather than pretending otherwise.

### 3.2 `F`/`T` are repeatable, though the Helix docs list only `f`/`t`

The Helix keymap table annotates `Alt-.` as *Repeat last motion (f, t, m, [ or
])*. Taken literally that excludes `F`/`T`. Two things decided it:

- acceptance criterion 3 says `Alt-.` repeats "the last find motion", and `F`
  and `T` are find motions;
- the doc parenthetical names `f`/`t` as representatives of the find family,
  which is how Helix's `Motion` enum is organised (`FindNext`, `FindTillNext`,
  `FindPrev`, `TillPrev`), and all four set `editor.motion`.

I did **not** execute Helix to confirm the runtime behaviour of `F` + `Alt-.`;
the argument above is from the documented family structure and the plan. All
four are recorded, which is the superset; a user who never writes `F` + `Alt-.`
loses nothing, and one who does gets the motion they just performed.

### 3.3 `:` and `/` outrank a pending target — stricter than Helix

Plan §4 requires `:` and `/` to keep outranking everything *including* a pending
find target, and acceptance criterion 6 restates it. Implemented in
`find_target_char`: with `NONE`, `:` and `/` are refused as targets and the key
is handed back for normal resolution.

This is stricter than taking the next key literally — in Helix `f` then `:` most
likely means find-colon. The counter-argument is recorded in §4.1 rather than
resolved here, because §14 makes a user-visible change a stop condition and the
plan is explicit.

With `SHIFT` they are ordinary characters: the `:` arm does not accept `SHIFT`,
so there is no modal line for them to open, and refusing them there would throw
away a usable target.

---

## 4. Requirements to evidence

| # | Requirement | Evidence |
|---|---|---|
| 1 | all four find motions work | `every_find_key_arms_and_its_target_resolves_across_lines` |
| 2 | `f`/`F`/`t`/`T` span line boundaries, unlike vi | `a_find_motion_spans_line_boundaries` (N1) |
| 3 | `t`/`T` land **on** the target | `till_motions_land_on_the_target_character`, `a_find_target_may_be_any_character_the_buffer_claims` (N2) |
| 4 | `Alt-.` repeats; another motion disarms | `alt_period_repeats_the_last_find_and_another_motion_disarms_it` (N7) |
| 5 | pending target consumes exactly one key | `every_pending_prefix_declares_its_leaf_and_its_unmatched_key_policy`, `every_find_key_arms_and_its_target_resolves_across_lines` |
| 6 | target never intercepts `:`/`/`, Insert-mode keys are not targets | `a_pending_find_target_yields_the_modal_line_keys` (N4), `insert_mode_never_captures_a_find_target` |
| 7 | a non-character key is handed back, not swallowed | `a_pending_find_target_hands_back_a_key_that_cannot_be_a_target` (N5) |
| 8 | multi-byte targets and UTF-8 boundaries respected | `a_multi_byte_target_matches_whole_characters_only` |
| 9 | motion past end-of-file | `a_find_motion_scans_strictly_past_the_cursor`, `a_find_beyond_the_scan_bound_is_disclosed_rather_than_called_absent` |
| 10 | the scan is bounded and discloses when it stopped | `a_find_beyond_the_scan_bound_is_disclosed_rather_than_called_absent`, `the_find_disclosure_distinguishes_absent_from_bounded_and_is_silent_on_success` (N6) |
| 11 | existing `gg` / `space y` behavior preserved | suite green; no `gg`/`space y` assertion edited |

### 4.1 The `:`/`/` argument, recorded not decided

The honest counter-argument: refusing `:` as a target means a user who actually
wants find-colon cannot get it, and `f` + `:` silently does something else.
Taking the key literally — Helix's reading — makes `f` `:` mean find-colon and
relies on `Esc` to escape an armed `f`.

The plan's rule was followed because it is stated as an invariant and
repeated in an acceptance criterion. Making the alternative is a user-visible
behavior decision, and §14 lists those as stop-and-report. This is left as a
small, self-contained future improvement rather than decided inside M003.

---

## 5. A defect found and fixed during the milestone

**`byte_at` cannot read a byte inside a multi-byte character.**

The first implementation of `find_char` walked the document one byte at a time
using the existing `byte_at` helper. That helper is
`read_bytes(offset, offset + 1).ok()`, and `DocumentSnapshot::read_bytes`
**refuses** any range that does not start and end on a character boundary. So
the byte at which a two-byte target begins is unreadable, `byte_at` returns
`None`, and the scan treated the character as width 1 and stepped past it.

It failed silently on ASCII — every ASCII document behaved correctly — and on
any document containing a non-ASCII character it walked past every multi-byte
target. This was not found by reading the code; it was found because
`a_multi_byte_target_matches_whole_characters_only` failed with `left: 0,
right: 1` on the input `"aé€a"`. That failure is the evidence.

The fix reads `FIND_SCAN_CHUNK`-sized windows with a
`target.len_utf8() - 1` overlap so a multi-byte target straddling a chunk edge
is still wholly inside one of them. `str::match_indices` yields byte offsets, so
no encoding is materialized and no character is allocated.

The performance effect was measured rather than assumed: the per-byte version
took **79 seconds** on the bound test; the chunked version takes **0.22 s**.

---

## 6. Non-vacuity

Every injection below was applied to a pristine copy of the file, the named test
was run, and the file was reverted and hash-compared byte-for-byte.

| # | Injected defect | Test that caught it |
|---|---|---|
| N1 | forward scan confined to `line_content_end` | `a_find_motion_spans_line_boundaries` — `editor.rs:3199` |
| N2 | `FindTillNext` resolves to `prev_char_boundary(offset)` | `till_motions_land_on_the_target_character` — `editor.rs:3249` |
| N4 | `find_target_char` no longer refuses `:`/`/` | `a_pending_find_target_yields_the_modal_line_keys` — `commands/editor.rs:5170` |
| N5 | a non-target key returns `Handled` instead of `Retry` | `a_pending_find_target_hands_back_a_key_that_cannot_be_a_target` — `commands/editor.rs:5207` |
| N6 | widget computes `find` as an empty `String` | `the_find_disclosure_distinguishes_absent_from_bounded_and_is_silent_on_success` — `components/editor.rs:1606` |
| N7 | `note_motion` returns unconditionally | `alt_period_repeats_the_last_find_and_another_motion_disarms_it` — `commands/editor.rs:5278` |

Plus the multi-byte defect in §5, observed as a real test failure before the
fix.

**One correction worth recording.** The harness initially scored N2 as
non-vacuous because the run exited non-zero — but the injected code called
`snapshot.prev_char_boundary`, which is not a method on `DocumentSnapshot`, so
the build failed and **no test ran at all**. That is a false positive: a
compilation error proves nothing about a test. The harness was changed to treat
`error[E…]` and "could not compile" as *inconclusive* and to require that a
named test actually reported `FAILED`, then N2 was re-injected with a
compiling body and genuinely failed the intended test. The lesson is recorded
because the failure mode is not exotic: any injection harness that keys only on
an exit code will eventually "prove" a test non-vacuous by breaking the build.

No new guard was added. M002's `check_tui_editor_key_authority.py` already
scopes to `handle_normal_key`'s key table and now reports **37** Normal-mode
chords (was 32: `f`, `F`, `t`, `T`, and `Alt` + `.`), with no shadowed arm.
That it needed no change is the evidence that `resolve_pending_prefix` stayed
outside that function.

---

## 7. One existing test changed

`modal_lines_open_and_unclaimed_keys_reach_the_composer` (M001) asserted that
`f`, `t`, `F` and `T` are unclaimed and therefore reach the composer. M003
claims all four, so they were removed from that list, with a comment saying so.

This is the shape that test was built for: it asserts which keys the buffer has
*not* taken, so claiming a key is supposed to require editing it. `y` stays on
the list because it is a leaf of `space` rather than a Normal-mode binding.

No other existing test was edited. No `gg` or `space y` assertion was touched.

---

## 8. Verification

All run locally on a tree containing the M003 changes.

| Check | Result |
|---|---|
| `python3 scripts/check_tui_editor_key_authority.py` | passed — 3 guard-block chords, 37 Normal-mode chords, 1 Insert-mode chord, no shadowed arm |
| `python3 scripts/check_tui_editor_prefix_state.py` | passed — one pending-prefix field (`pending_prefix`), no legacy mechanism |
| `python3 scripts/check_tui_editor_text_authority.py` | passed |
| `cargo test -p codegg --lib tui::` | **1907 passed, 3 failed** |
| `cargo test -p codegg --test tui_main_buffer` | 44 passed |
| `cargo test -p codegg --test tui_render` | 111 passed |
| `cargo test -p codegg --test interactive_terminal_tui` | 6 passed |
| `cargo fmt --check --all` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | one pre-existing error, attributed below |
| `scripts/verify.sh quick` | **passed, exit 0** |

### 8.1 The three `--lib` failures are not M003's

All three are in `src/tui/commands/interactive_terminal.rs`, which M003 does not
touch:

- `pressing_i_on_the_open_box_enters_focused_input` — **pre-existing**, proved
  in M002 by rebuilding at baseline `719cfeb5d` with the M002 changes stashed
  and observing the identical failure.
- `panel_rendered_buffer_shows_live_pty_output` and
  `box_rendered_buffer_shows_live_pty_output` — both **added by the unrelated
  terminal-panel work** that was restored into this tree at the end of M002
  after a stray `git stash pop`. Verified by locating both `fn` names as `+`
  additions in `git diff 719cfeb5d` for those files. That work has never been
  verified, and it does not currently pass.

### 8.2 The clippy error is not M003's

```
error: field assignment outside of initializer for an instance created with Default::default()
  --> src/tui/commands/interactive_terminal.rs:2565:9
   |
2565 |         session.id = "s-c001".to_string();
```

Same origin as §8.1: that line is a `+` addition in
`git diff 719cfeb5d -- src/tui/commands/interactive_terminal.rs`. It belongs to
the restored terminal-panel work, not to M003. Everything else in the workspace
is clippy-clean.

This means `scripts/verify.sh full` — which runs clippy with `-D warnings` —
cannot pass on this tree until that unrelated work is fixed. `verify.sh quick`
does not run clippy and does pass.

### 8.3 Not run

`scripts/verify.sh full` and `cargo nextest run --workspace`. Both are broad
sweeps outside this milestone's blast radius. `verify.sh full` is in any case
blocked by §8.2 for reasons unrelated to M003.

---

## 9. Terminal panel: unaffected

`cargo test -p codegg --test interactive_terminal_tui` — 6 passed. The terminal
panel keeps its own `prefix_pending`/`YankPrefix`; convergence remains out of
scope, as in M002.

---

## 10. Scope notes

**No protocol, storage, or daemon change.** No migration, no `STORAGE_LAYOUT_VERSION`
bump, no DTO. The `find_note` is `Copy` and bounded — three variants, at most one
`char` — so it adds nothing to any projection or remote DTO.

**No new guard.** M001's key-authority guard and M002's prefix-state guard both
already covered the shape M003 needed, and were deliberately not widened.

**`note_motion` is not folded into `editor_motion`.** `editor_motion` is also
Insert mode's cursor nudge; an arrow inside a sentence is not a gesture `Alt`-`.`
should forget. The rule therefore lives in the Normal-mode funnels —
`helix_motion` and the `gg` prefix leaf — which is two call sites and a
documented reason rather than one silent behaviour change.

**`apply_helix_verb` now carries two more fields back from its probe.** The
probe exists so a motion writes the real presentation exactly once; a find is
the one motion that also writes state about the *attempt*, and it resolves on
the probe. Both fields are seeded from the real presentation first, so a
non-find motion — which writes neither — cannot erase a record the dispatcher
deliberately kept.

---

## 11. Registry update and unblock audit

M003's own dependency (M002) was closed before this milestone began. The audit
of what M003's closure actually unblocks, against the roadmap's edges:

| Milestone | Dependency on M003 | State after this closure |
|---|---|---|
| M004 core change verbs | **hard** — shares the motion/verb resolution path | edge discharged; eligible for fresh planning |
| M005 textual motions + goto mode | **soft** | edge discharged; still gated on M002 (closed) — eligible |
| M010 space mode + extended goto | none (M005 soft, M002 hard) | unchanged; still needs M005 |
| M006, M011, M012 | none | unchanged — already "M002 edge discharged" |

**M004 is the one milestone M003 actually unblocks.** The roadmap's M003 entry
lists `Alt-.` as repeating "the last `f`/`t`/`m`/`[`/`]` motion"; `m` and
`[`/`]` leaves are M006's, so the repeatable set here is the find family only,
and M006 will widen it. That is the expected composition, not a defect.

Nothing else was newly unblocked. M007 remains independently available in
parallel; M008 stays blocked on M007; M009 stays blocked on M008 plus
ADR-0034 acceptance.

The plan's §15 unblock expectation is **not** met in full. It predicted M004,
M005 and M010 would all become ready; only M004 and M005 are actually
discharged, because M010's dependency is on M005, not on M003.

---

## 12. Residual findings, deliberately not acted on

- **`t` and `T` are currently indistinguishable from `f` and `F`.** See §3.1.
  The distinction is a selection property and belongs with M007. `docs/tui.md`
  says `as f` rather than implying otherwise.
- **`F`/`T` are repeatable although the Helix docs list only `f`/`t`.** See
  §3.2. Deliberate; the alternative would narrow a capability on the strength of
  a doc parenthetical.
- **`:`/`/` refusing to be targets is stricter than Helix.** See §3.3 and §4.1.
  A small, self-contained decision left for the maintainer.
- **`FindNote::label()` renders `no 'z' within the 2 MiB scan bound`.** The
  `2` is computed from `MAX_EDITOR_FIND_BYTES`, so changing the bound changes the
  string. `Bounded` is a status-row message, not a diagnostic.
- **`src/tui/components/help_overlay.rs` remains dead code** and still advertises
  bindings that do not exist. Carried forward from M002; unchanged here.
- **The shipped `/help` still lists no editor chords**, including these. The
  authoritative user-facing statement is `docs/tui.md`.
- **The restored terminal-panel work has 2 failing tests and 1 clippy error**
  (§8.1, §8.2). Not M003's, but it blocks `verify.sh full` on this tree and
  should be resolved before the next handoff.

---

## 13. Files changed by M003

Source (5 files):

- `src/tui/editor.rs` — `FindKind`, `FindMotion`, `FindOutcome`, `FindNote`,
  `MAX_EDITOR_FIND_BYTES`, `FIND_SCAN_CHUNK`, `find_char`, the two boundary
  helpers, the four `EditorCommand` variants and `find_motion()`,
  `EditorPrefix`'s four find variants with `find_kind`/`is_find`/`armed_by`,
  `apply_motion`'s find branch, `apply_helix_verb`'s probe carry-back,
  `clear_history`'s `forget_find`, and the model tests.
- `src/tui/document_session.rs` — `last_find`, `find_note`, `forget_find()`.
- `src/tui/commands/editor.rs` — `find_target_char`, the find branch in
  `resolve_pending_prefix`, `last_find`, `note_motion`, the four arms and
  `Alt`-`.` in `handle_normal_key`, and the dispatch tests.
- `src/tui/components/editor.rs` — the `find_note` widget field, the banner-row
  suffix, `Options`, and the render tests.
- `src/tui/app/render.rs` — passing `find_note.label()`.

Docs and planning:

- `architecture/tui.md`, `docs/tui.md`, `CHANGELOG.md`
- `plans/closure/tui-editor-helix-parity/003-status.md` (this record),
  `plans/registry.md`,
  `plans/subsystems/tui-editor-helix-parity-roadmap.md`,
  `plans/implementation/tui-editor-helix-parity/003-character-finding-motions.md`

No new guard script.