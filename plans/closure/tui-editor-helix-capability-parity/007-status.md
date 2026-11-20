# TUI Editor Helix Capability Parity Milestone 007 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-editor-helix-capability-parity/007-registers.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md#milestone-007--registers`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. There is no commit, branch or pull request
  carrying it, and the same tree carries M006 and M008 of this roadmap.

Labels: **measured** is what this record's author read out of the tree or ran;
**reported** is the implementing session's measurement, not repeated here.

## 1. Executive finding

Registers landed with the shape the plan recommended and with both decision points
resolved the way the plan recommended, and the observable behaviour is the
behaviour the plan specified.

**The store is sparse, typed and bounded.** `Registers` is
`{ named: BTreeMap<char, String>, unnamed: String }`
(`src/tui/document_session.rs:1008-1013`) — the plan's option *(i)*, chosen
because "a reader who yanks to `a` and `q` allocates two slots, not twenty-eight"
and because it does not trip the text-authority guard's `Vec<String>` rule.
`MAX_REGISTER_BYTES = 16 * 1024` (`:883`) is applied per entry on every write,
through `bounded_register_text`, which stops on a character boundary.

Four types carry the semantics rather than leaving them to call sites:
`RegisterName` is a **closed set** parsed once (`Unnamed`, `Blackhole`, `Search`,
`Named('a'..='z')`); `RegisterWrite` distinguishes `Stored` from
`Truncated { stored, requested }`, `Discarded` (blackhole) and `ReadOnly`
(search); and `RegisterRefusal` distinguishes `Blackhole`, `NoSearch` and
`Empty(name)`, each with its own sentence. That is the plan's "a refusal rather
than an empty string, because 'the blackhole holds nothing' and 'you never yanked
into `a`' are answers a paste owes the reader", implemented as a type.

**The prefix is one key deep, in the `m` grammar's shape.** `EditorPrefix` gained
`Register` and `RegisterNamed(name)` (`src/tui/editor.rs:319`, `:330`, `:387-388`),
`"` arms it, the next key is captured and validated by `resolve_register_name`
(`src/tui/commands/editor.rs`), an invalid name disarms and reports the set, and
the selection is consumed by the next keystroke. The plan explicitly rejected a
sticky field — "a selected register that survives an unrelated key is a second
grammar" — and no sticky field exists; the prefix-state guard still reports "one
pending-prefix field (pending_prefix), typed as EditorPrefix, no legacy mechanism
left" (measured, exit 0).

**D1 resolved to (a), and the consequence is documented in the user-facing
document.** `docs/tui.md`'s Registers section states: "**Bare `p` reads the
unnamed register**, which is seeded from the system clipboard when a document
opens and kept in step by every `y`. That is what lets `p` paste what you copied
in a browser while the buffer never reads the clipboard itself. It also means `d`
then `p` pastes the deletion — the behaviour Helix users expect, and the reason
registers exist … `y` still writes the system clipboard on every yank … `d` and
`c` never write it." The implementation matches: `seed_register_from_clipboard`
exists in the command layer (App-side), so the buffer never calls the clipboard,
and the tests named in §2 assert each leg.

**D2 resolved to (a): `Ctrl-r` stays resync, insert-register is unbound, and that
is documented.** "**Insert-mode register insertion is not bound.** Helix gives
`Ctrl+r` the insert-register gesture, and `Ctrl+r` here is the mode-independent
**resync** chord … every register is already reachable from `NOR` with `"`
followed by its name." Nothing was rebound, and the key-authority guard reports no
shadowed arm.

**The `space` family is not register-aware**, as the plan requires: `space y`,
`space p`, `space P`, `space R` remain the clipboard path, and
`space_p_after_a_delete_still_pastes_the_clipboard` asserts the separation
directly.

**One correction to the plan's own reasoning, recorded because it changes what
the guard evidence means.** The plan leans on the text-authority guard as its
*structural* test of the store's shape: "the guard treats a field holding
`Vec<String>` as a violation … a `Vec<String>` register store would fail the
build's guard step" (plan §7 A, and again in §16: "**The text-authority guard's
`Vec<String>` rule is the trap**"). Measured: until this tree's repair, that rule
**could not fire at all** — the pattern was `\bVec<String>\b`, and the trailing
`\b` after `>` can never match, because `>` is not a word character
(`git diff scripts/check_tui_editor_text_authority.py` shows the fix to
`\bVec<String>(?![A-Za-z0-9_])` with the comment "a trailing word boundary could
never match and this rule could not fire at all"). So the guard could not have
caught the shape the plan designed against, at the time the shape was chosen. The
store is nevertheless the right shape on its own merits, and the rule now bites:
the guard's `--self-test` (8 injected defects, measured) includes a
`Vec<String>`-shaped retained container that is rejected. The guard pass itself
was obtained with **no allowlist edit** — the diff is a strengthening (a
`--self-test`, a file-presence assertion, and the `\b` fix), not a widening.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A bounded register store with the unnamed register, `a`–`z`, `_` and `/` | `Registers { named: BTreeMap<char,String>, unnamed: String }`; `RegisterName::parse` covers exactly `"`, `_`, `/`, `a`–`z`; `every_valid_register_name_parses_and_the_set_is_closed` | pass | A closed set, parsed once. |
| The unnamed register is written by every yank **and** every delete, and read by bare `p` | `docs/tui.md`; `bare_y_then_bare_p_round_trips_through_the_unnamed_register`; `d_then_p_pastes_the_deletion_and_alt_d_leaves_the_register_alone` | pass | |
| A register is bounded and reaching the cap is disclosed, not silently truncated | `MAX_REGISTER_BYTES = 16 * 1024`; `bounded_register_text` stops on a character boundary; `RegisterWrite::Truncated { stored, requested }`; `a_register_write_is_bounded_and_the_truncation_is_reported` | pass | 16 KiB = 16384 bytes. |
| `"` arms a one-key-deep prefix; the next key is the register name | `EditorPrefix::Register`/`RegisterNamed` (`editor.rs:319`,`:330`); `Char('"')` → register prefix (`commands/editor.rs:6037`); `arm_register_name` | pass | |
| An invalid name disarms and reports the set; a non-character key is consumed | `resolve_register_name`; docs record `"D` as "a rejected register, not a delete" and `"` + `Tab` not toggling the composer | pass | |
| `"` then `Esc` leaves nothing selected | `EditorPrefix` disarm path (`editor.rs:497` and the disarm tests) | pass | Not separately named in this pass. |
| `y`, `p`, `P`, `R` honour the selected register | `yank_to_register`, `paste_from_register`, `apply_register_text`; `a_named_register_round_trips_and_the_yank_still_reaches_the_clipboard` | pass | |
| `d` and `c` yank into the **unnamed** register regardless of `"`, and `Alt-d`/`Alt-c` write nothing | `store_in_unnamed`; `d_then_p_pastes_the_deletion_and_alt_d_leaves_the_register_alone` | pass | M002's D1 discharged, and its clipboard test kept true. |
| `d`/`c` do **not** touch the system clipboard; `y` still does | `a_named_register_round_trips_and_the_yank_still_reaches_the_clipboard`; `Alt-d` case above | pass | The plan's §10 "migration" pair, both asserted directly. |
| The `space` family always means the system clipboard | `space_p_after_a_delete_still_pastes_the_clipboard` | pass | Not redirected by `"`. |
| The blackhole writes nothing and a read from it is a stated refusal | `RegisterWrite::Discarded`, `RegisterRefusal::Blackhole` with its message; `the_blackhole_register_discards_writes_and_refuses_reads`; `the_blackhole_register_discards_and_its_paste_refuses_with_a_reason` | pass | |
| The search register is a read-only **projection** of the search line, refusing before a search exists | `RegisterWrite::ReadOnly`; `read_register` delegates `/` to the presentation; `the_search_register_projects_the_search_line_and_refuses_a_read_before_one`; `an_unset_register_and_an_unsearched_slash_both_refuse_with_a_reason` | pass | Not a copy, so refining the line changes what `/` reads. |
| The store is cleared by the same transitions that clear the undo history | `opening_a_document_clears_every_register`; the field doc names `clear_history` as the single clearing point | pass | |
| One `y` over several members is one register entry | `one_write_is_one_register_entry_however_many_members_it_came_from` | pass | The plan's §10 contention item. |
| D1 (bare `p`, seed at open) | `the_unnamed_register_is_seeded_from_the_clipboard_at_open`; `seed_register_from_clipboard` in the App layer; docs record the rule and its cost | pass | |
| D2 (`Ctrl-r` stays resync) | no rebinding; key authority "no shadowed arm"; `docs/tui.md` states the deferral with its reason | pass | |
| The buffer never reads the clipboard; the seed is one App-layer call at open | `seed_register_from_clipboard(app, read)` takes the already-read value; `run_clipboard_verb` does the same for paste | pass | Clipboard authority stays with the App. |
| The text-authority guard passes with **no allowlist edit** | exit 0, "no retained text buffer and no direct filesystem reads in the editor path"; diff is a strengthening, not an allowlist | pass | §1's correction: the `Vec<String>` rule could not fire before the repair. |
| The key-authority guard reports no shadowed arm, with the Normal-mode figure moved | "2 guard-block chord(s), 92 Normal-mode chord(s) … 8 Insert-mode chord(s), no shadowed arm" (measured) | pass | M005's record recorded 90 at its close; the delta is not attributed here. |
| The leaf guard's expectations updated for the new `"` arm | "6 leaf table(s), 52 leaf glyph(s), 15 prefix binding(s), 4 unavailable leaf reason(s)" (measured) | pass | 15 prefixes at M005's close was reported as 13. |
| The doc-drift guard passes in both directions | "52 leaf gestures and 100 chords are documented, and every key declared unbound is unbound" (measured, exit 0) | pass | M005's record recorded 98 chords. |
| No migration, protocol variant, crate or new authority | `Registers` is `TuiDocumentPresentation` state; no `DocumentSnapshot` owned; no manifest or protocol file touched by this milestone | pass | |
| `cargo test -p codegg --lib tui::editor` green | **discharged 2026-11-13** by `scripts/verify.sh full` | pass | §10; 17,377 workspace tests, 0 failures. |
| `bash scripts/verify.sh quick` green | **discharged 2026-11-13** by `scripts/verify.sh full`, which calls `quick` | pass | §10. |

## 3. Production implementation evidence

- `src/tui/document_session.rs:875-1060` — `MAX_REGISTER_BYTES`, `RegisterName`,
  `RegisterWrite`, `RegisterRefusal` (with `message()`), `Registers` and its
  `write`/`read`/`clear`, plus `bounded_register_text` and the unnamed-register
  seed. The field doc on `jumplist`'s sibling states the "not a second text
  buffer" contract in the type's own words.
- `src/tui/editor.rs:319-388` — `EditorPrefix::Register` / `RegisterNamed(name)`,
  their pending labels (`"` and `"a`), and `awaits_register_name`/`register_name`
  accessors.
- `src/tui/commands/editor.rs` — `arm_register_name`, `resolve_register_name`,
  `register_name_char`, `resolve_register_verb`, `yank_to_register`,
  `paste_from_register`, `apply_register_text`, `seed_register_from_clipboard`,
  `run_clipboard_verb` (the unmodified `ClipboardVerb` path).
- `docs/tui.md:1648-1725` — the Registers section: the prefix, the register set,
  which verbs honour a register, the 16 KiB bound and the 27-slot total, the
  clearing rule, the blackhole refusal, the search register's projection, the
  D1 seed rule and the D2 deferral.

Nothing in the plan's "explicitly out of scope" list was implemented: `Ctrl-r`
insert-register is unbound, there is no `%`/`#`/`:`/`.` register, registers are not
durable, the store is not shared across documents, there is no inspector UI, and
`space y`/`space p` are unchanged.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_text_authority.py --self-test
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_tui_editor_prefix_state.py

git diff scripts/check_tui_editor_text_authority.py
grep -rn 'MAX_REGISTER_BYTES|Register' src/tui/editor.rs
sed -n '860,1060p' src/tui/document_session.rs
sed -n '1648,1725p' docs/tui.md
```

### Results

| Command | Result |
|---|---|
| `check_tui_editor_text_authority.py` | **pass** — "no retained text buffer and no direct filesystem reads in the editor path", exit 0 |
| `check_tui_editor_text_authority.py --self-test` | **pass** — "the unmodified copy is accepted and all 8 injected defects are rejected", exit 0 |
| `check_tui_editor_key_authority.py` | **pass** — 2 / **92** / 8, no shadowed arm, exit 0 |
| `check_tui_editor_leaf_authority.py` | **pass** — 6 tables, 52 glyphs, **15** prefix bindings, 4 unavailable reasons, exit 0 |
| `check_tui_editor_doc_drift.py` | **pass** — 52 leaf gestures, **100** chords documented, exit 0 |
| `check_tui_editor_prefix_state.py` | **pass** — one `pending_prefix`, typed `EditorPrefix`, exit 0 |
| `MAX_REGISTER_BYTES` | `= 16 * 1024` at `document_session.rs:883` (measured) |
| `EditorPrefix::Register` | present, with `RegisterNamed(char)` (measured) |
| Store shape | `BTreeMap<char, String>` + `unnamed: String` (measured) |

### Not run

| Command | Why |
|---|---|
| `cargo test -p codegg --lib tui::editor` | Not run at the time of writing: compiling was expensive and three sessions were compiling, by instruction. **Discharged 2026-11-13** by the `scripts/verify.sh full` workspace sweep (17,377 tests, 0 failures). |
| `cargo test --test tui_render` | Same. **Discharged 2026-11-13**: render corpus 1,237 passed, 0 failed. |
| `cargo fmt --check`, clippy, `bash scripts/verify.sh quick` | Out of scope and forbidden for that session. **Discharged 2026-11-13** by `scripts/verify.sh full`, which runs `quick` → `run_guards()` (first line `cargo fmt --check --all`) and clippy `-D warnings`. |

## 5. Invariant review

- **A register is bounded, and reaching the cap is disclosed.** Holds — 16 KiB,
  `Truncated { stored, requested }`, and a notice at write time.
- **Registers are frontend-local and lifecycle-cleared.** Holds — the field doc
  names `clear_history` as the single clearing point, and
  `opening_a_document_clears_every_register` asserts it.
- **`Alt-c`/`Alt-d` never touch a register or the clipboard.** Holds, and is the
  assertion M002 wrote — kept true by this milestone, which is the whole reason
  the test is named for both behaviours.
- **The `space` family always means the system clipboard.** Holds, asserted
  directly.
- **The blackhole writes nothing and reads refuse.** Holds — a `RegisterWrite`
  variant and a `RegisterRefusal` variant, not an empty string.
- **No second text buffer.** Holds; and the plan's structural test for it now
  actually bites (see §1's correction and §10).
- **No dead chord; `Ctrl-r` stays resync; one owner per chord.** Holds — the
  key-authority guard reports no shadowed arm and nothing was rebound.
- **No mutation outside `apply_local`.** Holds: a register paste builds the same
  `EditorEdit` the clipboard verbs build, through the same funnel, so the
  read-only refusal applies unchanged.

## 6. Failure and recovery review

- **Partial failure.** A yank that cannot read one member skips it, as
  `yank_editor_ranges` already did before this milestone; a register write is one
  entry for the whole gesture (`one_write_is_one_register_entry_however_many_members_it_came_from`).
  A rejected paste transaction is reported and leaves the document unchanged.
- **Duplicate delivery / cancellation races.** None; writes are synchronous over
  the snapshot the yank already holds, and no async work was added.
- **Restart.** Registers are empty after a restart by construction — presentation
  state, not persisted. Recorded because the plan requires the record to say so.
- **Stale generation.** Cleared by the same transitions as the undo history; and a
  paste from an emptied register refuses with "register `a` is empty" rather than
  pasting nothing.
- **Bound reached.** The write truncates and discloses; a later paste from a
  truncated register is still a paste, and the truncation was already disclosed at
  write time.
- **Register selected but the store cleared.** The selection lives on the
  presentation, so a clear removes it with the store; a paste then refuses rather
  than silently falling back to the unnamed register.

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency migration. Two behaviour
changes, both decided rather than discovered, and both documented in `docs/tui.md`:

1. **`d` and `c` now yank** into the unnamed register. They still do not touch the
   system clipboard, so a user's clipboard is never clobbered by a delete. This is
   M002's D1 discharged.
2. **`p` reads the unnamed register**, which is seeded from the clipboard at open
   and kept in step by `y`. Under D1(a) this is indistinguishable from before for a
   user who only copies and pastes; it differs only after a `d`, where it now
   pastes the deletion.

D2's compatibility surface is **none**, because nothing was rebound: `Ctrl-r`
remains the mode-independent resync chord. Removal criteria for a transitional
state: none; both changes are the intended end state.

## 8. Security review

No authorization change: a register write reads bytes the daemon already
authorized through the snapshot the yank holds, and a register paste goes through
`apply_local` like every other edit, inheriting the read-only refusal.

The properties that matter are bounded and enforced in the type:

- **Bounded storage.** 16 KiB per entry × 27 possible slots, so the store cannot
  grow into a copy of the document.
- **No clipboard authority in the buffer.** The seed is one App-layer read at
  open; a per-paste read would have put clipboard authority in the buffer, which
  the plan's stop conditions forbid.
- **No retained document text.** The text-authority guard's rules are the test,
  and the store's shape is a map of bounded fragments rather than a document
  buffer.

## 9. Documentation and operations

- `docs/tui.md:1648-1725` — the complete Registers section, including the two
  behaviour changes, the bound, the refusal messages, the `space`-family
  separation, and the insert-mode deferral with its reason.
- The `space ?` keymap box: the doc-drift guard passes in both directions
  (52 leaf gestures, 100 chords), so the box, the tables and the document agree.
- `scripts/check_tui_editor_text_authority.py` — gained a `--self-test` and a
  file-presence assertion in this tree (the parent's repair), which is the
  operational check that its rules keep applying to files that still exist.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's structural guarantee — "a `Vec<String>` register store would fail the build's guard step" — **was not in force** when the store was designed: the guard's pattern was `\bVec<String>\b`, whose trailing word boundary after `>` can never match. The rule now works (the repair differs by one lookahead and the `--self-test` proves it with a `Vec<String>` case). | The milestone got the right shape for the right reason, but the evidence the plan cites for it was vacuous at the time. A future reader who trusts that sentence would assume a test that did not run. | Nothing to change in the store. Record the repair's provenance where the plan makes the claim, and treat "the guard would have caught it" as a claim requiring a `--self-test` case before it is cited. |
| low | D1 and D2 resolved to (a) and (a), recorded in `docs/tui.md`, but the plan and roadmap still read as open recommendations. | Same documentation gap as M006: a settled decision looks undecided to a planning reader. | Add one resolution line per decision point to the plan and the roadmap. Parent-owned. |
| low | The plan's `"ap` after `"ay` "in a different document is refused because the store is per-document" is delivered by *clearing on open* (`opening_a_document_clears_every_register`) rather than by a named cross-document test. | Only one document is attached at a time, so the case is structurally unreachable except through a switch — and a switch clears. The behaviour is right; the test name the plan implies does not exist. | Accept the clearing test as the evidence, and say so, or add an explicit test that opens two documents in sequence. |
| low | **Discharged 2026-11-13.** `scripts/verify.sh quick`, `cargo fmt --check` and both test lanes were not run by this record's author; the tree was uncommitted. | The plan's §11 list had no result in this record. | None: the 2026-11-13 `scripts/verify.sh full` ran the workspace sweep (17,377 tests, 0 failures), the render corpus (1,237, 0 failures), `quick` → `run_guards()` and clippy `-D warnings`, all green on this tree. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

Implementation is complete against all nine acceptance bullets, both decision
points are resolved and recorded where a user and a maintainer read, the guards
pass with no allowlist widening, and M002's D1 obligation is discharged with
M002's own clipboard test kept true. The operational condition is **discharged**:
`scripts/verify.sh full` ran on this tree at 2026-11-13 — 17,377 workspace tests
and 8,441 feature-pass tests, 0 failures, `cargo fmt --check --all` and clippy
`-D warnings` clean, render corpus 1,237 passed — and `full` calls `quick`, which
calls `run_guards()`. What remains are the two evidence items above (the
text-authority pattern-repair provenance and the cross-document register
naming), which assert behaviour the code already has, and the plan/roadmap
D1/D2 resolution lines, which are documentation edits rather than milestone
defects.

What M008 may rely on unchanged: `Registers`' shape and cap, the `"` prefix, the
unnamed-register seed, and the fact that `d`/`c` yank into it — which is exactly
what M008's macro storage needs.

## 12. Registry updates

- `plans/registry.md` line 30 (subsystem row): "M006 in flight, M007–M009 pending"
  → M007 conditionally closed with this record's path.
- `plans/registry.md` line 222 (the M007 milestone row): status "**ready**" →
  conditionally closed, with this record and the D1(a)/D2(a) resolutions named.
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §12 row
  `M007 registers`: `ready for handoff` → `conditionally closed`, closure record
  `plans/closure/tui-editor-helix-capability-parity/007-status.md`, blocker "the
  cross-document register test or its documented substitute" (the
  `quick`/`fmt --check` half was discharged 2026-11-13).
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` §7 M007 body:
  record D1(a) and D2(a) as resolved.
- `plans/implementation/tui-editor-helix-capability-parity/007-registers.md`
  header: `Status: ready for handoff` → `Status: conditionally closed`. **This
  record's author made this header edit**; the D1/D2 resolution lines are left for
  the parent because they are plan content.

## Correction log (2026-11-13)

- **§2, the two acceptance rows** — "`cargo test -p codegg --lib tui::editor`
  green | not run" and "`bash scripts/verify.sh quick` green | not run" →
  discharged by the 2026-11-13 `scripts/verify.sh full` run: 17,377 workspace
  tests and 8,441 feature-pass tests at 0 failures, and `full` calls `quick`.
- **§4, `Not run`** — the three rows (both test lanes; `fmt --check`, clippy,
  `quick`) → each marked discharged by the same run, which includes
  `cargo fmt --check --all` and clippy `-D warnings`.
- **§10, low (`quick`/`fmt`/tests not run)** — "Run on the merged tree from the
  merging session" → discharged 2026-11-13; the run happened on this tree.
- **§11** — "The condition is operational: green `quick` and `fmt --check` on
  the merged tree, and the two small evidence items above" → the operational
  half is discharged; the two evidence items and the plan/roadmap D1/D2 lines
  remain named.
- **§12, the roadmap blockers cell** — "green `quick`/`fmt --check` on the
  merged tree" → discharged 2026-11-13; the blocker that remains is the
  cross-document register test or its documented substitute.
