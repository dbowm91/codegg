# Closure record — TUI editor Helix parity Milestone 019

Milestone: **M019 — the open-line verbs: `o` and `O` open a line**
Plan: `plans/implementation/tui-editor-helix-parity/019-open-line-verbs.md`
Roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M019,
Amendments A1 and A2)
Closed: 2026-10-20
Baseline: `8df5dbb32` (`feat/auto-20261018-12255c45`)

## 1. The finding, in four facts

1. **The keys were bound.** `src/tui/commands/editor.rs` routed `o` to
   `EditorCommand::OpenLineBelow` and `O` to `EditorCommand::OpenLineAbove`.
2. **The route dropped the edit.** `enter_insert` maps a command to a cursor
   position through a match whose only arms are the four *insertion entry*
   commands; the two open-line commands fell to `_ => cursor`. The function is
   named for what it does — it moves the cursor and hands over the keyboard —
   and an open-line command has no cursor position of its own, so the match had
   nothing to say about it. The result: `o` entered Insert mode on the line
   the reader was already on, and no line was opened.
3. **The correct implementation was written, tested, and unreachable.**
   `editor::resolve_edit` had real arms for both commands, delegating to
   `open_line_below` / `open_line_above`, and two tests asserted the text it
   produces. Those tests call `resolve_edit` **directly**, so they never
   touched the key path — the helper-path-only shape the roadmap's §9
   prohibits for a binding.
4. **The documentation already described the fixed behaviour.**
   `docs/tui.md` said *"`o` opens the line **and** hands you the keyboard,
   while `]Space` inserts the line and leaves the cursor exactly where it
   was"*. The code did not open the line, so the document and the
   implementation disagreed — and the document was right about what the key
   should do.

`EditorEdit::stay_insert` was documented as *"Whether the controller accepting
this edit should keep the editor in insert mode (**open-line commands**,
insertion entry)"*. The flag was designed for exactly this verb and had never
been used by it.

## 2. What was built

| Work package | Where |
|---|---|
| W1 — the acceptance signal | `src/tui/commands/editor.rs`: `submit_edit` returns `bool` — whether the controller accepted the transaction |
| W2 — the open-line route | the same file: `editor_open_line`, beside `enter_insert` |
| W3 — the two arms | the same file: the `o` and `O` arms of `handle_normal_key` |
| W4 — the `stay_insert` invariant, restated | the same file: `submit_edit`'s mode write, and the comment that claimed an invariant this milestone made false |
| W5 — a typing run starts where the cursor ends | the same file: `typing_run_continues`, and `submit_edit`'s absorption condition |
| The open-line cursor | `src/tui/editor.rs`: `open_line_below`'s cursor, and its doc comment stating the geometry |
| The tests | the same file's test module: five dispatch-path tests, and two existing tests whose contracts this milestone changed |

## 3. The model

### 3.1 The route

`editor_open_line` is **not** a second funnel. It shares
`editor_editing_command`'s read-only gate, its snapshot and its
`resolve_edit`, and differs only in what it returns:

| Outcome | Return | Why |
|---|---|---|
| the attachment cannot be written | `BufferKey::Consumed` | the verb is claimed and the refusal is **reported**, as `d`/`D`/`x` do; `Unhandled` would drop the `o` into the composer |
| the controller accepted the edit | `BufferKey::EnterInsert` | the document took the keyboard |
| the controller refused the edit | `BufferKey::Consumed` | `submit_edit` has already reported why |

The two verb families are now distinguished, and the distinction is
principled rather than convenient. `i`/`a`/`I`/`A` are **mode transitions**:
they move the cursor and nothing else, so they leave the key unclaimed and the
status row's "read-only" is the report. `o`/`O` **write**, so they claim the
key and report the refusal as a notice. Both families keep the property the
old test was really asserting — the keyboard is not handed over.

### 3.2 The acceptance signal

`submit_edit` returns whether the controller accepted the transaction. That is
the only honest answer to "did the document take this edit?", and the
open-line verbs need it: a mode transition decided on a rejected transaction
would leave the editor in Insert mode over an unchanged document, which is a
worse state than the one this milestone repairs.

The acceptance check is also what keeps the presentation's mode and the App's
`input_mode` in agreement. `submit_edit` writes the presentation's side of it
inside the accepted branch; `editor_open_line` returns `BufferKey::EnterInsert`
only when that branch was taken, so the App adopts Insert mode in exactly the
case where the presentation already did.

### 3.3 The open-line cursor

The cursor goes to the **start of the opened line**, and the two cases reach
it by different arithmetic, because the opened line is bounded by different
newlines:

- with a terminator to split, the insertion is one newline at the start of the
  *next* line, and that newline is the one that ends the opened line — so the
  opened line starts where the insertion did;
- with no terminator to split, the insertion is a full blank line — two
  newlines — and the first ends the current line while the second ends the
  opened one, so the opened line starts one byte further on.

**The pre-existing cursor was one byte past the opened line** in the first
case. That is invisible until the reader types: `o` followed by "xy" produced
`one\n\nxytwo\n` — the word went into the line *after* the one that was
opened. The defect was found by the new dispatch-path test, not by reading the
code, and `open_line_below`'s doc comment now states the geometry so the next
reader does not have to re-derive it.

### 3.4 The undo granularity

The line `o` opens and the word typed into it are **two** undo steps: `u`
takes back the word, and a second `u` takes back the line.

That is the consequence of three things, each of which is now recorded:

1. **The insert session is a typing run.** M012's model is that everything
   typed in one go in `INS` is one entry. The `o` is not typed — it is an edit
   made in `NOR` that then hands the keyboard over.
2. **The geometry forbids merging them.** The open-line edit inserts its
   newline at the **end** of the opened line while the cursor sits at the
   **start** of it, so the typing goes in before the newline. A run is one
   contiguous insertion by construction.
3. **The model was asked the wrong question.** `submit_edit` folded an edit
   into a run when the editor was in Insert mode *after* the edit — and the
   open-line edit is what puts it there. A run whose text does not end where
   the cursor is cannot be continued, so the first keystroke broke the run and
   the word was split across two entries: the burst was **three** steps, not
   two.

The fix asks the question the run model already asked of every later edit, of
the first edit too: **does the cursor end where the inserted text ends?**
`typing_run_continues` asks it, and the open-line edit — whose cursor sits at
the start of the line its own newline ends — is not a typing edit. It is its
own undo step; the typing that follows opens the run.

The check is on the **cursor**, not on the mode. A mode check would also have
excluded the open-line edit, but it would have excluded the *literal*
insertion too, and the literal insertion in Normal mode is how the model's own
burst test (`a_rapid_edit_burst_serializes_with_one_change_in_flight`) drives
a run. The cursor check asks the question the run model actually cares about,
and it leaves the literal insertion alone — that test still passes unmodified.

## 4. The guards

No new guard. This milestone changes no chord, no leaf, and no dispatch shape,
so the existing guards cover it:

- the key authority guard still reports **2 guard-block / 71 Normal / 8
  Insert**;
- the leaf authority guard still reports **5 tables / 44 glyphs / 12
  bindings**;
- the completion, apply-path, prefix-state and text-authority guards are
  untouched.

The **doc-drift guard** that arrived with M020 is the one that would have
caught this milestone's root cause: the documentation had described the fixed
behaviour for the whole time, and nothing compared the two.

## 5. Verification

Every number below is from the sweep against this tree
(`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass — see the note below |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo build --locked --bin codegg-sandbox-helper` | pass |
| `cargo test -p codegg --lib --locked tui::commands::editor::tests::` | **231 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::editor::tests::` | **182 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::` | **2317 passed / 0 failed** |
| `cargo test -p codegg --lib --locked` (whole lib) | **6842 passed / 3 failed / 4 ignored** (1 filtered out) |
| the three failures, each run alone | **1 / 0**, **1 / 0**, and the `interactive_terminal` module **94 / 0** |
| the filtered asset-refresh test, alone | **1 passed / 0 failed** |
| `cargo test -p codegg-protocol --locked` | **259 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked` | **1012 passed / 0 failed** |
| the five editor guard scripts | pass; counts unchanged at **2 / 71 / 8** and **5 / 44 / 12** |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

**The formatting note, stated plainly.** The sweep's first run of the
`cargo fmt --all -- --check` step ran against the tree *before*
`cargo fmt --all` was applied, and it **failed**. The formatting was applied
and the check re-run: **clean**. Every step after the first ran against the
formatted tree, and the re-run check is the one reported above.

### 5.1 The three failures, and what they are

The whole-lib step failed with **3** failures, and none is this milestone's:

| Failure | Alone | What it is |
|---|---|---|
| `tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop` | the module is **94 / 0** | the pre-existing **load-sensitive** `interactive_terminal` class |
| `tui::commands::interactive_terminal::tests::the_badge_names_insert_while_the_terminal_box_takes_keystrokes` | **1 / 0** | the same class |
| `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` | **1 / 0** | the pre-existing **load-sensitive** `core::daemon` failure |

Both classes are in files this milestone does not touch
(`src/tui/commands/interactive_terminal.rs`, and the `core::daemon` module),
and both pass in isolation. The `interactive_terminal` class was already
registered as a residual in M017's closure (§5.2 there, with a four-run
evidence table); the `core::daemon` one is in the same registered set. **Which**
members of the `interactive_terminal` class fails varies between runs — M017's
sweep saw `a_live_terminal_surface_repaints_with_no_further_user_input` and
this one saw `the_badge_names_insert_while_the_terminal_box_takes_keystrokes` —
which is itself the signature of a load-sensitive failure rather than a
regression.

**A closure states the new failure set, not the total.** The new failure set
is empty: this milestone's five new tests pass, and the two existing tests
whose contracts it changed pass with the new contract asserted.

## 6. Exit conditions

The plan's §8 acceptance criteria, and the evidence for each:

| # | Criterion | Evidence |
|---|---|---|
| 1 | `o` opens a line below the cursor's line and enters Insert mode; `O` opens a line above — **through the key path** | `o_opens_a_line_below_the_cursor_and_takes_the_keyboard`, `capital_o_opens_a_line_above_the_cursor_and_takes_the_keyboard` |
| 2 | the cursor lands **on the line that was opened**, not on the line after it | the same two tests assert the cursor is 4, and `open_and_add_newline_agree_on_text_and_differ_on_mode` now asserts 4 with the geometry spelled out |
| 3 | a read-only attachment refuses `o` and says so; the document is unchanged and the key does not reach the composer | `o_on_a_read_only_attachment_is_refused_visibly`, and `every_insertion_entry_is_reachable_through_dispatch` (which now separates the two verb families) |
| 4 | `]Space` and `[Space` are unchanged | `o_and_close_space_stay_different_verbs` asserts the text, the mode and the `BufferKey` for `]Space` |
| 5 | the key guard's counts do not move | **2 / 71 / 8** and **5 / 44 / 12**, before and after |

The plan's §6 test list asked for five dispatch-path tests. Five were added,
and one of them — the last-line case — is a **measurement** rather than an
assertion of a chosen shape, because the shape at the last line is the
pre-existing rule's business and not this milestone's.

## 7. Recorded divergences

Three, all forced by evidence the plan did not have. Each is recorded in the
plan's §12 rather than left implicit, and each was measured by a test rather
than reasoned about.

| # | The plan said | What the code said |
|---|---|---|
| 12.1 | the pre-existing `open_line_below` is reused unchanged | its **cursor** was one byte past the opened line, and the reader's typing went to the line after it |
| 12.2 | a read-only refusal returns `BufferKey::Consumed` | the existing test asserted `Unhandled` for `i`, `o` and `A` alike, so the two verb families were covered by one assertion that could only hold for one of them |
| 12.3 | `o` and the typing are one undo step | they are **two**, and the model made it three before the fix |

## 8. The documents that moved

| Document | What moved |
|---|---|
| `docs/tui.md` | the change-verbs section: `o`/`O` open a line, the read-only refusal, and the two-step undo; the read-only list now names `o` and `O` |
| `architecture/tui.md` | the `stay_insert` paragraph: the open-line verbs' route, the acceptance signal, and the cursor geometry; the undo section: `typing_run_continues` and why the question is on the cursor |
| `CHANGELOG.md` | two entries under **Fixed** — the keys that did not open a line, and the burst that was split |
| `.opencode/skills/tui/SKILL.md` | two new invariants (the run's first edit, the open-line route) and the guard table, which was **missing three rows** and now carries five |

The skill and its `architecture/` doc moved in the same commit, as `AGENTS.md`
requires.

## 9. Unresolved findings

| Severity | Finding | State |
|---|---|---|
| **medium** | `o` and the word typed into it are two undo steps, and a reader may reasonably expect one | **registered, not discharged.** Making it one needs the open-line edit and the typing to merge into a single contiguous insertion, and they are not contiguous: the newline the `o` opened the line with sits *after* the text the reader types. That is a property of the geometry, not of the history model, and changing the history model is a separate decision for the maintainer. |
| **medium** | the load-sensitive `interactive_terminal` class (2 of the 3 whole-lib failures) and the load-sensitive `core::daemon` failure (the third) | pre-existing, registered in M017's closure §5.2, not this milestone's. Each passes in isolation: the `interactive_terminal` module is **94 / 0**, and the `core::daemon` test is **1 / 0**. |
| **low** | `i`/`a`/`I`/`A` refuse a read-only attachment without a notice, leaving the status row as the only report | **registered, not fixed.** The refusal *is* visible — the status row reads "read-only" — and `enter_insert`'s doc comment now says so accurately. The milestone's own draft of that comment called it silent; the comment was wrong and was corrected. |
| **low** | the `Alt+` / `Alt-` spelling inconsistency in `docs/tui.md` | **registered, not fixed.** The doc-drift guard (M020) accepts both spellings deliberately; fixing the style is a documentation decision, not a binding one. |
| **low** | `o`/`O` on a last line with no terminator appends a full blank line, so the file gains a trailing newline | pre-existing shape of `open_line_below`, unchanged by this milestone, and now reachable. Recorded in the plan's §12.1 as the one place the opened line's geometry is not Helix's. |

## 10. Carried forward

- **The change quartet's missing data source** — `]g`/`[g`/`]G`/`[G` still has
  none: `egggit`'s `ChangedFile { path, kind }` and `FileDiff { path, patch }`
  carry no line ranges. It needs its own milestone and its own plan.
- **The dirty-buffer merge** — unchanged and still deferred.
- **The other LSP pickers show the first rows only** — diagnostics, locations,
  symbols, highlights, signature help, rename files. M017 overrode the
  standing scroll for the code-action panel alone.
- **The code-action read passes no diagnostics context** — a server that keys
  quick fixes to the diagnostics it was told about offers fewer here.
- **M014** stays blocked on ADR-0034's acceptance.

## 11. Roadmap disposition

M019 is **closed**. It is the milestone the roadmap's §11 completion
definition needed: the `o`/`O` gap was the last real Helix binding in this
editor that did nothing, and it was found by reading the *document* against
the *code* — which is the drift M020's guard now makes mechanical.

The roadmap's §11 is amended by Amendment A1 (M020) and the roadmap closes
with M019 and M020 as its last two milestones. See M020's closure record §11.

## 12. Registry updates

- `plans/registry.md`: the M019 row moves to `closed`, with this record
  linked.
- The roadmap's §12 table gains an M019 row.
- The roadmap's §7 gains the M019 entry, with Amendment A1 (ready) and
  Amendment A2 (closed).
- The blocked-work audit found nothing to unblock: M014's blocker is
  ADR-0034's acceptance, which this milestone does not touch, and no other
  registered plan lists M019 as a dependency.
