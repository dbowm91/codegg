# TUI Editor Helix Parity Milestone 009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/009-match-mode-and-text-objects.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M009), §13 (stage C)
Repository baseline reviewed: `cad651c18` (M012, wave 2 stage B)

---

## 1. Executive finding

M009 is **closed**. The editor has Helix's `m` grammar: `m m` jumps to the
mate of the bracket under the cursor, `m s`/`m d`/`m r` surround, unsurround
and re-surround the selection, and `m a`/`m i` select a text object — a word,
a WORD, a paragraph, a delimiter pair, or the innermost pair around the
cursor. The status row shows which half of the grammar the reader is in, and
a bracket match that cannot land says **which** of the two things went wrong.

Five things in this milestone are worth recording beyond the checklist, because
each is a decision a later milestone would otherwise have to re-litigate.

1. **Neither of the plan's two `PrefixAction` options was taken, because
   both were aimed at the wrong question.** §6.2 offered a `Verb(HelixVerb)`
   payload (dropping `Copy`) or a second field on the presentation (breaking
   the prefix-state guard). The leaf table does not need to carry a *verb* at
   all: `m s`, `m r`, `m d`, `m a` and `m i` all say the same thing — **which
   argument is being waited for** — so the leaf carries
   `PrefixAction::Capture(MatchArg)`, whose payload is a `Copy` enum, and the
   verb is produced when the *capture* resolves. `Copy` was never lost, the
   leaf tables stayed `const`, and the presentation still has one prefix
   field. See the plan's §17.1.
2. **The two halves of `m r` had to be told apart on screen.** They are the
   same word — `m r` — until the first character of the replacement pair is
   captured, and a pending prefix a reader cannot predict is a prefix they
   will get wrong. `EditorPrefix::display()` therefore changed from
   `&'static str` to `Cow<'static, str>`, and the second half renders
   `"m r ("`. That is five bytes, which is exactly the indicator bound the
   plan already had — the bound was chosen for the longest label and now has
   one. This answers the roadmap's §13.4 question 4.
3. **A post-edit offset cannot be read from the pre-edit snapshot, and the
   first implementation tried.** `clamp_offset` pulls an offset past the old
   end of a document back to the old end, so a member built by reading the
   pre-edit text with a post-edit offset covered one character too few — and
   the surround's selection looked like it had not grown. The offsets are now
   **computed**: the closing delimiter's own start, or the content's last
   character in pre-edit coordinates moved by the shift the edit causes. See
   the plan's §17.9.
4. **A bracket at the very end of a file was invisible to the matcher.** The
   editor's `next_char_boundary` helper returns `None` for the **last**
   character of a document as well as for the end of one — a behaviour its
   own test asserts, and one that eleven call sites depend on. The new
   `char_at` uses `char_end`, which already draws the distinction, so `m m`
   on a `)` at the end of a file finds its mate. The helper itself was left
   alone: changing a shared helper's semantics in a keybinding milestone is
   exactly the drift this programme's closure records exist to prevent. See
   the plan's §17.11.
5. **The plan's `PrefixAction` frame was wrong about where the payload
   belonged, and the plan's object table was loose about what a WORD is.**
   §6.3 calls `W` "the whitespace-delimited run", but the editor's own
   `W`/`B`/`E` motions use `is_long_word_byte`, which is *not whitespace* —
   so a WORD is everything up to whitespace, punctuation included, and on
   `a.b` the two objects differ. The object follows the **motion**, because
   the object must agree with the `W` the reader just used to get there. See
   the plan's §17.12.

---

## 2. The work packages that landed

| Package | What it delivered |
|---|---|
| W1 | `src/tui/editor_match.rs` — the delimiter table (`PAIRS`: `(`, `[`, `{`, `<`, `'`, `"`, `` ` ``) and the pair scans, dependency-free |
| W2 | The grammar: `EditorPrefix::Match`, `EditorPrefix::MatchArg(MatchArg)`, `MATCH_LEAVES`, the `m` arming arm, and the `m`-grammar capture with its own capital rule |
| W3 | `EditorCommand::MatchBracket`, the bounded scan window, the three refusals, and the `MatchNote` disclosure |
| W4 | The five object resolvers — word, WORD, paragraph, delimiter pair, closest pair — and `HelixVerb::SelectObject` |
| W5 | The three surround verbs, the post-edit selection, and the shared edit funnel |
| W6 | The guards, the docs, and the closure |

The W1 module grew two more scans while W4 was implemented:
`innermost_pair` (the innermost pair **of a given kind**) and `closest_pair`
(the innermost pair **of any kind**), because `enclosing_pair` — the module's
original scan — finds the **outermost** one, and the two answer different
questions. `m r` needs the outermost (so the nesting survives) and `m i m`
needs the innermost.

---

## 3. The model

### 3.1 One prefix field, two levels

The presentation still has exactly one pending-prefix field, so
`scripts/check_tui_editor_prefix_state.py` is green. The sub-state rides in
the `EditorPrefix` variant: `EditorPrefix::Match` is the first level and
`EditorPrefix::MatchArg(arg)` is the second, with `MatchArg` naming the
argument being waited for.

The alternative — a second `pending_*` field — is exactly what the
prefix-state guard exists to prevent, and the guard's injection proof
(§5.2) shows it still fires on that shape.

### 3.2 The leaf says which argument, not which verb

`MATCH_LEAVES` pairs a glyph with a `PrefixAction`, and the two-level
commands' leaves carry `PrefixAction::Capture(MatchArg)`. The verb is
produced at **capture resolution** time, in the command layer, where the
captured character is in hand.

This is why `PrefixAction` and `PrefixLeaf` are still `Copy` and the leaf
tables are still `const`: the payload is a `Copy` enum, not a `HelixVerb`
(which is not `Copy`, because the regex verbs carry a `String`).

### 3.3 Three refusals, not one

`m m` can fail in three distinguishable ways, and the reader's next move
differs in each:

| Refusal | What it says | What it means |
|---|---|---|
| no bracket | "no bracket under the cursor" | move the cursor |
| unbalanced | "no matching bracket" | the code is unbalanced |
| bounded | "the answer is not known" | the file is too big to answer |

The disclosure lives on `TuiDocumentPresentation::match_note`, beside the
find and paragraph notes, and `lift_motion_notes` lifts it into the notice
immediately after the motion — so it describes exactly one keystroke. The
lift now reads the paragraph and bracket notes **together**, because only one
motion can have run, and two separate lifts would let the second clear the
first's words.

### 3.4 The surround verbs edit, and then hold a selection

`submit_edit` clears every selection on success, because a selection is a set
of byte offsets into text that no longer exists. The surround verbs mean to
leave one behind — `m s` grows it over the new delimiters — so
`HelixVerbOutcome` gained `EditAndSelect(EditorEdit, Option<String>,
Vec<Selection<EditorPoint>>)`, and the command layer applies the members
**after** the submission.

The members are in post-edit coordinates, and they are computed rather than
read (§1.3).

### 3.5 `m d` and `m r` disagree about which pair they mean, on purpose

`m r` replaces the **outermost** enclosing pair, so the nesting inside
survives: `(a(b))` becomes `[a(b)]`, not `[a(b])`. `m d` removes the
**innermost** pair, which is the one immediately around the selection, so
unwrapping is local. The asymmetry is real and is recorded rather than
smoothed over: each rule is the one that makes its own acceptance criterion
true.

### 3.6 A text object is a selection, and it is gated like one

`m a` and `m i` enlarge what the reader holds, so they are refused on a
read-only attachment for the same reason `v` and `x` are.
`grows_selection` gained `HelixVerb::SelectObject`; `mutates_buffer` gained
the three surround verbs.

---

## 4. The scans: byte-derived, bounded, and honest about which pair

**Every scan runs inside a window.** The window is at most
`MAX_SET_VERB_READ_BYTES` (256 KiB) — the same bound every other set verb
obeys, and the bound the roadmap's exit condition names — centred on the
cursor and snapped to character boundaries at both ends. The window's offsets
are window-relative and are mapped back with the window's base.

The window is a **bounded slice**, never a copy of the document. It is built
per keystroke and dropped at the end of it. Its field is named `chunk` rather
than `text`, because `scripts/check_tui_editor_text_authority.py` rejects a
struct field named `text` outright, and the honest name for a bounded window
is not the name of a document.

**`m m` is a delimiter-table match, not a syntax match.** Helix marks it
`TS`; this editor counts brackets over bytes. The divergence was already
recorded in the roadmap's Amendment A3 and is repeated in the docs.

**The quote's mate is the next unescaped quote.** A quote is both an opening
and a closing delimiter, so "the mate" is ambiguous; the next quote is the
reading every editor gives a string delimiter.

---

## 5. The guard-injection proof

Each injection is applied, the guard is run, the file is restored, and the
restoration is checked **byte-identical**. The proof is only evidence if the
guard would actually have complained, so each case below is the shape the
guard is supposed to catch.

### 5.1 Key authority — a duplicated `m` arming arm

`src/tui/commands/editor.rs` gets a second `(NONE, Char('m'))` arm. The
guard reports **rule 2** (two arms claiming the same chord) and exits 1.

### 5.2 Prefix state — a second prefix field

`TuiDocumentPresentation` gets a second `pending_*`-shaped field. The guard
reports "the editor must have exactly one pending-prefix mechanism" and
exits 1. This is the M009-specific regression shape, and the one the
milestone's whole design is built to avoid.

### 5.3 Prefix state — the `m` grammar's leaf table renamed away

`MATCH_LEAVES` is renamed. The one-field rule still holds and the capture
classifier still holds, so **every other assertion in the guard is
satisfied** — and the grammar is gone. The guard's required-declaration list
now names `MATCH_LEAVES`, `pub enum MatchArg`, `PrefixAction::Capture` and
`EditorPrefix::MatchArg`, so it reports the absent declaration and exits 1.

This is the shape the plan's §6.8 asked for: "the plan should add the new
names to the guard's required-declarations list so the grammar cannot be
deleted silently". Without it, the M009 grammar could be deleted and the
guard would still be green.

**The guard's required-declaration check moved to comment-free text.** The
first version of this list was checked against the raw source, and a **doc
comment** mentioning a name satisfied it — `EditorPrefix::Match`'s own doc
comment names `MATCH_LEAVES`. So the grammar could have been deleted and the
guard would still have been green, with the prose left behind as the only
evidence. The check now runs against the same comment-stripped text the
forbidden-name scan uses, and the injection above renames **every**
occurrence — including the doc comment's — to prove it.

That is a change to a guard, and the guard's **output** is unchanged: the
required list is a set of names whose presence is checked, not a count, so
adding names does not move the "one pending-prefix field" line.

### 5.4 Text authority — the window's field renamed to `text`

`ScanWindow`'s field is renamed from `chunk` to `text`. The guard reports the
retained-text violation and exits 1.

All four: **fired, exit 1, restored byte-identical**. The working tree after
the proof carried only the milestone's own changes.

---

## 6. The verification and test evidence

### 6.1 The sweep

| Command | Result |
|---|---|
| `cargo fmt --all && cargo fmt --check` | clean |
| `CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0, no warnings |
| `CARGO_BUILD_JOBS=2 cargo build --locked --bin codegg-sandbox-helper` | exit 0 |
| `CARGO_BUILD_JOBS=2 cargo test -p codegg --lib tui:: --locked` | **2272 passed / 2 failed** — see §6.3 |
| `bash scripts/verify.sh quick` | see §6.4 |
| `python3 scripts/check_tui_editor_key_authority.py` | 2 guard-block / **70** Normal-mode / 8 Insert-mode chords |
| `python3 scripts/check_tui_editor_key_authority.py --self-test` | 10 cases, all pass |
| `python3 scripts/check_tui_editor_prefix_state.py` | one pending-prefix field, green |
| `python3 scripts/check_tui_editor_text_authority.py` | green |
| `rustc --edition 2021 --test src/tui/editor_match.rs` | **41 passed / 0 failed** (was 33) |

The Normal-mode chord count moves **69 → 70**: the `m` arming arm is one
chord. The guard counts chords, not gestures, and the plan counted 66 (a
figure already stale by the time M009 was written, because M011 and M012 each
added chords of their own).

### 6.2 The tests

**25 new tests**, taking the `tui::` library suite from 2249 to 2274:

| Where | How many | What they cover |
|---|---|---|
| `src/tui/commands/editor.rs` | 16 | the grammar end to end: arming, retrying an unlisted glyph, `m m` in both directions, both refusals, the pair interior, the innermost pair, the word and the WORD, the capital's two spellings, the object vocabulary's retry, the three surrounds, and the two read-only refusals |
| `src/tui/editor_match.rs` | 8 | `innermost_pair` and `closest_pair`: the nearest enclosing pair, a type the cursor is outside of, an unclosed delimiter, a quote's parity, the tightest pair of any kind, the innermost of one kind, no pair at all, and the shared budget |
| `src/tui/editor.rs` | 1 | every `MatchArg` is a capture, the two halves of `m r` do not render alike, and the restricted vocabulary is exactly the two object arguments |

Three existing tests were **extended** rather than replaced: the prefix table
(the `Match` row), the indicator-bound list (the seven `m` rows, and
`ReplaceChar`, which the list had been missing — §17.14 of the plan), and the
component's "no two prefixes render identically" test (the eight `m` rows).

### 6.3 The two failures are the known load-sensitive pair, and they pass alone

The two failures are
`tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop`
and `tui::commands::interactive_terminal::tests::the_badge_names_insert_while_the_terminal_box_takes_keystrokes`.
Both are the **load-sensitive `interactive_terminal` pair** M012's §6.5
already recorded, and neither has anything to do with this milestone: they
assert on a *mounted* terminal surface, and the surface is gone by the time
the assertion runs when the machine is busy.

Run in isolation: `cargo test -p codegg --lib interactive_terminal --locked`
→ **94 passed / 0 failed**.

This milestone therefore does **not** claim a green `cargo test -p codegg
--lib` run. It claims the 2272 that passed, the two that failed and why, and
the fact that both pass alone.

### 6.4 `verify.sh quick`

**"Quick verification passed."** The routine subset — the agent schema, the
core/client/desktop boundaries, the sandbox, execution ownership, the TUI
authority guards, the HTTP route disposition, the audit coverage, the
scheduler bypass, the config merge coverage, the no-hardcoded-models check,
the provider wire/catalog/resilience guards, the OpenAI endpoint guard, the
Eggwork routing guard, the first-party skills check, the shell-policy
safety check and its self-test, and `cargo check --workspace --all-targets`
— all passed.

### 6.5 The source-injection proof

Each injection breaks one production behaviour, the named test is run, and
the failure is recorded; the file is then restored byte-identical.

| Injection | The test that must fail |
|---|---|
| the `m` arming arm removed | `m_arms_the_match_prefix_and_the_indicator_shows_it` |
| the `m` capture stops reconciling capitals | `m_i_w_reads_a_capital_sent_as_a_shifted_lowercase` |
| `char_at` goes back to `next_char_boundary` | `m_m_jumps_to_the_mate_of_the_bracket_under_the_cursor` |
| the surround-add selection stops growing | `m_s_surrounds_the_selection_and_grows_it_over_the_new_delimiters` |

All four: **the named test failed**, and the file was restored
byte-identical.

---

## 7. The exit conditions, with their evidence

| Exit condition | Evidence |
|---|---|
| bracket matching survives unbalanced and multi-byte input | `m_m_on_an_unbalanced_delimiter_refuses_with_the_unbalanced_reason`; the module's own multi-byte tests; the scan is byte-based and boundary-snapped |
| surround replace preserves nesting | `m_r_replaces_the_outermost_pair_and_keeps_the_inner_ones` — `(a(b))` → `[a(b)]` |
| objects resolve against a bounded window and never scan the whole buffer per keystroke | `ScanWindow::around` is capped at `MAX_SET_VERB_READ_BYTES` and centred on the cursor; `closest_pair` shares **one** budget across every pair type |
| the prefix machine keeps one field | `check_tui_editor_prefix_state.py` green; the injection in §5.2 fires |
| the guards stay honest | the key-authority guard's count moves to 70 and its self-test is 10/10; §5's three injections all fire |
| the docs describe what the code does | `docs/tui.md`, `architecture/tui.md`, `CHANGELOG.md` and `.opencode/skills/tui/SKILL.md` all moved in the same commit |

---

## 8. The recorded divergences

1. **`m m` is a delimiter-table match, not a syntax match.** Helix marks it
   `TS`. Recorded in the roadmap's Amendment A3, in the plan, and in
   `docs/tui.md`. A syntax-aware `m m` is M014's slice.
2. **The object vocabulary is the parser-free one.** `w`, `W`, `p`, a
   delimiter pair and the closest pair. The syntax-node objects (`f`, `t`,
   `a`, `c`, `T`, `x`) are M014's slice and are registered separately, so
   they are not orphans.
3. **A quote's mate is the next unescaped quote.** Recorded, because "the
   mate" of a delimiter that is both an opener and a closer is ambiguous.
4. **`m d` is the innermost pair and `m r` the outermost.** Recorded in the
   plan's §17.7 and in `docs/tui.md`.
5. **`m m` is not repeatable with `Alt-.`.** `note_motion` records a motion
   in the repeat slot only for the motions it knows how to re-issue, and
   `MatchBracket` is not one of them. Helix supports it. Recorded as a
   limitation; the repeat slots were not widened for it, because a repeat
   slot is a **semantic** claim about which motions are re-issuable, and
   that is a milestone of its own.

---

## 9. Notes carried forward

1. **`next_char_boundary`'s doc and its tested behaviour disagree.** The doc
   says it returns "`None` only when `offset` is already the end"; the
   implementation and its test say it is also `None` for the **last
   character** of a document. The tested behaviour is the contract and
   eleven call sites depend on it, so the helper was not changed. The doc
   comment is wrong and should be corrected by the milestone that owns the
   helper — and any new caller must reach for `char_end`, not
   `next_char_boundary`, when it wants the end of the last character.
2. **`capture_char` does not reconcile capitals.** The `m` grammar's
   capture does, because its object vocabulary contains `W`. The find and
   replace captures take a character as data and have no case to reconcile.
   A pre-existing difference, recorded rather than fixed.
3. **The find capture refuses `:` and `/`; the `m` capture refuses them too.**
   The same rule, for the same reason, written twice — once in
   `capture_char` and once in `match_arg_char`. The duplication is
   deliberate: the two rules will have to diverge when the find capture
   changes, and the milestone that changes it should change both.

---

## 10. The residual

- **M014** remains blocked on ADR-0034 and deliberately has no plan. It owns
  the syntax-node object slice and a syntax-aware `m m`.
- **The 69 failing `tests/` integration binaries** are pre-existing and
  untouched by this milestone. They were not repaired here and are not
  claimed to be repaired.
- **`m m` is not repeatable with `Alt-.`** See §8.5.
- **A surround acts on the primary selection only.** A multi-selection
  surround is a different gesture and is not offered; the plan says so and
  the implementation follows it.
- **A text object resolves against the cursor, not the selection.** There
  is no selection-shaped text object, and the plan says so.
- **A cursor on a blank line has no paragraph under it,** and the refusal
  says so rather than guessing which neighbour was meant.

---

## 11. Handoff

The next milestone is **M010** (space mode, the pickers, and the clipboard
family) — wave 2 stage D. It inherits:

- one pending-prefix field, with the `m` grammar's second level riding in
  the `EditorPrefix` variant;
- a `PrefixAction::Capture(MatchArg)` arm, which is the shape any future
  two-level grammar should use;
- a `HelixVerbOutcome::EditAndSelect` variant, which is the shape any future
  "edit, then hold a selection" verb should use;
- the bounded scan window, which is the shape any future "look at the text
  around the cursor" resolver should use;
- and the guard at **2 / 70 / 8**, with the chord count to be recomputed
  again when M010 claims its keys.
