# TUI editor Helix parity Milestone 008 — Multi-selection command surface

Status: implemented — **closed 2026-10-19**. All eighteen verbs are bound and
the key-authority guard moved from 48 to 66 Normal-mode chords, the count §10
of this plan pinned. Closure:
`plans/closure/tui-editor-helix-parity/008-status.md`.
Predecessor status — ready for handoff, unblocked 2026-10-18: M007 closed and
its §5 set API shipped exactly as pinned below, so this plan needed no
corrective pass. Contract verification:
`plans/closure/tui-editor-helix-parity/007-status.md` §11.
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M008
Long-term requirements: `plans/000-long-term-specification.md` (TUI editor);
`plans/002-long-term-roadmap.md` (editor capability sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 0. Why this plan is written before its dependency closes

M008's hard dependency is **M007**, which is not closed. Per
`plans/003-planning-process.md` §4 a milestone is dependency-ready only when every
hard dependency is closed, so **this plan is registered `blocked`, not `ready`**,
and the registry names M007 as the blocker.

It is written anyway, for one specific reason: **M008's verbs are undefined until
the set API they operate on exists.** Writing it now lets it do the only thing
that can be done in advance — pin down the exact API contract M007 must expose,
so M007 and M008 cannot drift into two incompatible shapes the way
`EditorPrefix::leaf()` → `leaves()` had to be corrected once already. §5 is that
contract and is the part a reviewer should read first.

The registry entry and this header must carry the same label. A `ready` here
would be a governance defect, not an optimisation.

## 1. Objective

Land the Helix verbs that only make sense over a selection set — the layer that
makes CodeGG a multi-cursor editor rather than a single-selection one.

| Key | Helix label | Command |
|---|---|---|
| `;` | Collapse selection onto a single cursor | `collapse_selection` |
| `Alt-;` | Flip selection cursor and anchor | `flip_selections` |
| `Alt-:` | Ensures the selection is in forward direction | `ensure_selections_forward` |
| `%` | Select entire file | `select_all` |
| `,` | Keep only the primary selection | `keep_primary_selection` |
| `Alt-,` | Remove the primary selection | `remove_primary_selection` |
| `C` | Copy selection onto the next line (Add cursor below) | `copy_selection_on_next_line` |
| `Alt-C` | Copy selection onto the previous line (Add cursor above) | `copy_selection_on_prev_line` |
| `S` | Split selection into sub selections on regex matches | `split_selection` |
| `K` | Keep selections matching the regex | `keep_selections` |
| `Alt-K` | Remove selections matching the regex | `remove_selections` |
| `(` | Rotate main selection backward | `rotate_selections_backward` |
| `)` | Rotate main selection forward | `rotate_selections_forward` |
| `Alt-(` | Rotate selection contents backward | `rotate_selection_contents_backward` |
| `Alt-)` | Rotate selection contents forward | `rotate_selection_contents_forward` |
| `&` | Align selection in columns | `align_selections` |
| `Alt-minus` | Merge selections | `merge_selections` |
| `Alt-_` | Merge consecutive selections | `merge_consecutive_selections` |

All eighteen rows are quoted from `https://docs.helix-editor.com/keymap.html`,
§Normal mode → Selection manipulation. **The plan binds Helix's semantics, not
the roadmap's prose** — the M004 `R` correction established that rule, and this
table is the guard against repeating it.

## 2. Why this milestone is ready (once M007 closes)

- **M007 — closed** (hard). The set, the primary, and the range-set payloads.
  Until then every verb below is undefined.
- **M001 — closed**, transitively: the key-authority guard is what proves
  eighteen new chords did not shadow or duplicate anything.
- No ADR gates this scope. Every verb is frontend-local; `S` / `K` / `Alt-K`
  reuse an existing workspace dependency rather than adding one.

## 3. Current implementation evidence

### No chord collides — verified against the guard's own parser

The mode-independent guard block runs at the top of `editor_buffer_key`
(`src/tui/commands/editor.rs:2795`), before mode dispatch. It claims exactly
**two** chords: `Ctrl+s` → `BufferKey::RequestSave` (`:2842`) and `Ctrl+r` →
`BufferKey::RequestResync` (`:2845`).

`handle_normal_key` (`:2882`) declares **48** chords: `Ctrl+d`/`Ctrl+u`, `hjkl`
+ arrows, `PageUp`/`PageDown`, `0`, `$`, `G`, `g`, `w`/`b`/`e`, `W`/`B`/`E`,
`f`/`F`/`t`/`T`, `Alt-.`, `v`, `x`, `d`, `y`, `p`/`P`, `R`, `~`, `` ` ``,
`` Alt-` ``, `r`, `D`, `Space`, `[`, `]`, `u`/`U`, `i`/`a`/`I`/`A`/`o`/`O`, `:`,
`/`, `n`/`N`. The only `ALT` claims today are `Alt-.` (`:3025`) and
``Alt-` `` (`:3100`).

Running `python3 scripts/check_tui_editor_key_authority.py` reports
*"2 guard-block chord(s), 48 Normal-mode chord(s), 1 Insert-mode chord(s), no
shadowed arm."*

**Verdict: all eighteen M008 chords are unclaimed.** Bare `c`, `s`, `C`, `S`,
`K`, `;`, `,`, `%`, `&`, `(`, `)` are free. This is the second-cleanest keymap
addition in the roadmap after M004's, and it means no existing binding moves.

### Regex is already available — no new dependency

`regex = "1"` is declared in `[workspace.dependencies]` (`Cargo.toml:49`) and
consumed by the root package (`Cargo.toml:213`). Workspace members using it:
`crates/codegg-providers/Cargo.toml:37`, `crates/codegg-core/Cargo.toml:34`
and `:65`, `crates/eggsentry/Cargo.toml:20`.

**`src/tui/` already uses it**: `src/tui/components/messages.rs:26-29`
(`Lazy<regex::Regex>` for URL and path detection) and `src/tui/command.rs:1317`,
`:1325`. `S` / `K` / `Alt-K` need **no new dependency and no new workspace
policy**. No `fancy-regex`, `onig`, or `regress` exists anywhere — do not reach
for one.

### `&` has no alignment primitive — it is built from scratch

There is **no padding or column-formatter** in the repository. What exists is
width *measurement*:

- `display_width(snapshot, range)` — `src/tui/editor.rs:2841`
- `bounded_display_width(...) -> (usize, bool)` — `src/tui/editor.rs:2857`
- `longest_visible_width(...)` — `src/tui/editor.rs:2872`
- `column_width(&self, placement: PanelPlacement)` — `src/tui/layout.rs:174` —
  **panel placement, not content**
- `display_width` / `display_width_str` — `src/tui/chat_browser.rs:674`,
  `src/tui/components/editor.rs:1157`

So `&` must write its own padding logic. It must reuse `bounded_display_width`
to stay inside the 32 KiB / 256 KiB budgets documented at
`src/tui/editor.rs:2866-2871`, and it must disclose truncation the way
`line_content_end` and `first_non_whitespace` do. `&` can honestly promise width
measurement and padding; it cannot promise anything about byte alignment, and
`EditorPoint` ordering is by `byte`, not by display column — so members on lines
containing tabs or CJK will align by **display width**, which is a decision to
make explicitly rather than discover.

### `%` is cheap

There is no whole-buffer range helper, but building one needs only two existing
primitives: `line_start(snapshot, 0)` (`src/tui/editor.rs:1246`) and
`buffer_content_end(snapshot)` (`:1315`). Both are offset lookups, not content
scans, so `%` over a multi-megabyte file needs **no bounded-window compromise**
for range construction. Bounded concerns apply to rendering and to the regex
verbs only.

### Insert-mode integrity has a real gap

`handle_insert_key` (`src/tui/commands/editor.rs:2587`) has a catch-all at
`:2589-2594`: `KeyCode::Char(ch) if key.modifiers == NONE || SHIFT =>
editor_insert_literal(...)`.

- `;` `,` `%` `S` `K` `C` `&` `(` `)` are therefore literal text in Insert today,
  and adding Normal-mode arms does not change that.
- **`ALT`-modified characters are NOT covered by that arm.** `Alt-;`, `Alt-,`,
  `Alt-C`, `Alt-K`, `Alt-(`, `Alt-)`, `Alt-minus`, `Alt-_` currently fall through
  Insert entirely. That is pre-existing, but this milestone introduces **eight**
  `ALT` chords, so the question becomes real: should `Alt-;` in Insert type `;`,
  or stay inert? Helix's Insert mode binds only the eleven chords listed on the
  keymap page, none of them `Alt-` punctuation, so **inert is the faithful
  answer** — but it must be asserted, not assumed.

Nearest existing coverage:
`tests/modifier_chords.rs:296` `uppercase_direction_letters_are_ordinary_text_in_insert_mode()`,
`tests/modifier_chords.rs:411` `subagent_tab_letters_are_ordinary_text_in_insert_mode()`,
`tests/tui.rs:1910` `test_app_shift_punctuation_inserts_text()`.
**No exhaustive printable-integrity test exists.** That is a gap, and this
milestone widens it — §6.

### Guards need no change

`scripts/check_tui_editor_key_authority.py` already accepts `ALT` as a
first-class modifier (`MODIFIERS`, `:106`), and both spellings parse — a
`let m = … if m == alt` binding (`:466-472`) or a literal `KeyModifiers::ALT`.
Write `Alt-minus` as `(m, KeyCode::Char('-')) if m == alt` and `Alt-_` as
`(m, KeyCode::Char('_')) if m == alt`.

`scripts/check_tui_editor_prefix_state.py` constrains only
`TuiDocumentPresentation`'s `*prefix*` fields (`:226-241`). **The regex prompt
must not introduce one** — see §6.

`scripts/check_tui_editor_text_authority.py` is unaffected: no retained text
buffer, no filesystem read.

## 4. Invariants that must not regress

1. **Unclaimed-key fallthrough.** Eighteen new Normal-mode chords narrow the
   composer's keyspace. That narrowing is intended; the fallthrough contract
   must still hold for everything else.
2. **Insert-mode text integrity.** Every new glyph, and every new `ALT` chord,
   must be proven not to be swallowed in Insert (§3's gap).
3. **One transaction per edit.** `&`, `Alt-(`, `Alt-)`, `C`, `Alt-C`,
   `Alt-s`/`S` all mutate. They land as **one** `apply_local` transaction —
   M007's §4.1 rule, inherited.
4. **Bounded work.** No verb may scan the whole buffer per keystroke. `regex`
   is given a bounded subject slice, not the entire document.
5. **Text authority.** No filesystem read, no process execution. In particular
   `S` and `K` match **in-buffer text only**; they are not shell filters.
6. **One prefix field.** A regex prompt must not add a `*prefix*` field to
   `TuiDocumentPresentation` (§6).
7. **Terminal-panel parity.** M007 claims parity is preserved by construction;
   this milestone adds chords, and a chord that also reaches the panel's
   Normal-mode table would be a divergence. §12.
8. **No existing binding moves.** `R` is `replace_with_yanked`, reload is `:e`,
   `D` is cursor-only. None is revisited here.

## 5. The M007 API contract this milestone depends on

M007's set API must expose at least this. If M007 closes without it, this
milestone is blocked on a **shape** gap rather than a milestone gap, and the fix
is a corrective pass against M007 — not a local workaround.

**Reads**

- `selections() -> &[Selection<P>]` — document order.
- `primary() -> Option<Selection<P>>` / `primary_index() -> Option<usize>`.
- `selection_point_of(index)` — for a per-member motion.
- `last_motion() -> Option<Motion>` and a way to **set** it, because M003's
  `Alt-.` record must learn the new motions (§7, `C`/`Alt-C` and `%` are motions).

**Writes**

- `set_members(Vec<Selection<P>>, primary) -> bool` — the single constructor for
  every verb in §1. Returns whether the set changed, so a no-op can be reported
  instead of submitted.
- `set_primary(index) -> bool`
- `insert_at(point, mode) -> bool`, `insert_at_relative(index, line_delta)` —
  for `C` / `Alt-C`.

**Operations** — all on `SelectionState` or as free functions over the set, and
all **pure** (they build an edit and never apply it):

- `collapse_to_primary`, `flip_all`, `ensure_forward`, `keep_primary`,
  `remove_primary`, `rotate_primary(delta)`, `rotate_contents(delta)`,
  `merge_all`, `merge_consecutive`, `align_columns`, `split_on_regex`,
  `keep_matching`, `remove_matching`.

**Invariants they must preserve**

- Members stay sorted by `bounds()` and the primary stays attached to its member
  across every one of them.
- Every operation that changes nothing returns `false` / a no-op result rather
  than pushing an undo entry. *An identity mapping must not push an undo entry
  that undoes nothing* — the M004 `case_range` rule.
- `set_members` with an empty vec clears the set; with one member is legal and
  means "one selection".

## 6. Required production changes

### `S` / `K` / `Alt-K` — a regex prompt

Helix's labels say "matching **the regex**", and Helix prompts for the pattern.
CodeGG has a `:` command line (`EditorCommandName::ALL`, nine commands) but no
generic argument prompt. Two options:

1. **Reuse `:`.** Add three command names (`split-regex`, `keep-regex`,
   `remove-regex`) that read the pattern from the command line. No new prompt
   surface; discoverable through the existing palette.
2. **A dedicated argument prompt.** Cleaner UX, but a new input mode, a new
   pending state, and a new disarm contract.

**Recommendation: option 1.** It adds no prefix, no input mode, and no new state
to disarm on focus change or reload — and the prefix-state guard's one-field rule
stays trivially satisfied. If option 2 is chosen instead, the prompt state must
**not** be a `TuiDocumentPresentation` field whose name contains `prefix`;
otherwise `scripts/check_tui_editor_prefix_state.py:226-241` fails, and weakening
that guard to accommodate a prompt would undo M002's central invariant.

The compiled regex must be bounded: a bounded subject slice per member, and a
complexity guard, because a pathological pattern over a multi-megabyte
document is a freeze. Report a rejection rather than hanging.

### The remaining verbs

- `;` — keep the primary, collapse to a cursor at its head.
- `Alt-;` — swap `anchor` and `head` on every member.
- `Alt-:` — normalise every member so `anchor <= head`. Must be idempotent and
  must not move the cursor.
- `%` — `line_start(0)..buffer_content_end` as a single member, primary.
- `,` / `Alt-,` — keep / remove the primary. Removing the last member leaves an
  empty set, which must report rather than panic.
- `C` / `Alt-C` — copy each member onto the next / previous line. Each added
  cursor is a **new member**, and the set stays sorted. Over the 256-edit ceiling
  (§4.3) refuse with a notice.
- `(` / `)` — move which member is primary. **`Alt-(` / `Alt-)` rotate the
  *contents***, which is a buffer edit, not a selection change. These are the
  two most commonly confused pairs in the layer and need separately named tests.
- `&` — measure each member's display width via `bounded_display_width`, pad to
  the maximum, apply as one transaction, disclose truncation.
- `Alt-minus` — merge every member into the primary's range.
- `Alt-_` — merge **only adjacent** members; leave gaps alone.

### Insert-mode coverage

Extend the sweep to cover all eighteen glyphs in both `NONE` and `SHIFT`
encodings, plus the eight `ALT` chords, asserting each is **inert** in Insert.

## 7. Deferred, each recorded

The roadmap's M008 boundary omits several Helix selection verbs. They are real,
they are not in the boundary, and folding them in silently would be scope creep:

| Verb | Helix label | Why deferred |
|---|---|---|
| `s` | Select all regex matches inside selections (`select_regex`) | Not in the boundary; would double the regex surface. Natural sibling of `S`, so it should be planned **with** it rather than after. |
| `Alt-s` | Split selection on newlines (`split_selection_on_newline`) | **Cheaper than `S`** and needs no regex. If `S` ships, this is the obvious next slice. |
| `_` | Trim whitespace from the selection (`trim_selections`) | Not in the boundary; cheap. |
| `X` / `Alt-x` | Extend / shrink selection to line bounds | Not in the boundary; interacts with M007's `SelectionMode`, so it may belong to the milestone that finalises line-mode semantics. |
| `J` / `Alt-J` | Join lines inside selection (`join_selections`, `join_selections_space`) | Not in the boundary; a real edit. |
| `Ctrl-c` | Comment/uncomment the selections (`toggle_comments`) | Not in the boundary; needs a comment path to exist. M010's `space c` mentions "if a comment path exists". |
| `Alt-o`/`Alt-up`, `Alt-i`/`Alt-down`, `Alt-p`/`Alt-left`, `Alt-n`/`Alt-right`, `Alt-a`, `Alt-I`/`Alt-Shift-down`, `Alt-e`, `Alt-b` | Syntax-tree selections | ADR-0034, proposed. |
| `v`-as-extend-mode unification | — | The roadmap says fold into M008 "if it proves bounded, otherwise a corrective addendum". Recorded, undecided. |

`Alt-.` should learn `C` / `Alt-C` and `%` as repeatable motions (they are
motions in Helix's sense), consistent with M006's widening and M003's record.

## 8. Failure, cancellation, restart, contention semantics

- **No members held.** Every verb reports and does nothing. "Nothing is
  selected" and "`C` did not work" must be distinguishable — the M004 clipboard
  precedent applies.
- **A no-op must be reported, not submitted.** `,` when one member is held, `;`
  when one member is held, `Alt-:` on an already-forward set. Each pushes no undo
  entry and says so once.
- **Regex that does not compile** — reported, buffer untouched.
- **Regex that matches nothing** — the roadmap's exit condition calls this out
  explicitly: "regex verbs handle a no-match set without corrupting the buffer".
  For `S` that means the set is unchanged; for `K`/`Alt-K` it means the set
  becomes empty (and reports). Neither may corrupt.
- **Above 256 members or edits** — refuse with a notice naming the limit.
- **Prompt cancelled** — `Esc` aborts with no set change and no undo entry.
- **Buffer switch, reload, focus change** — the regex prompt must disarm
  exactly as `g`, `space`, and `[`/`]` do. A pending pattern must never apply to
  a newly opened buffer.

## 9. Compatibility and migration

No protocol, storage, or configuration change. No new dependency: `regex` is
already in the root package and already used under `src/tui/`. Adding three
`:` command names extends `EditorCommandName::ALL`, which is user-visible in the
palette and must be documented.

## 10. Required tests

- **Every one of the eighteen verbs, dispatch-path**, driven through
  `editor_buffer_key` — never by calling a helper directly. That discipline is
  what let the `Ctrl+r` shadow survive M001.
- Each verb against a **heterogeneous set**: mixed empty/non-empty members,
  mixed `Char`/`Line` modes, members on different lines, members on the same
  line. The roadmap's exit condition names this and it is the only test shape
  that would catch a verb assuming uniformity.
- `S` / `K` / `Alt-K`: matches, no matches, invalid pattern, a pattern matching
  inside a multi-byte member, a bounded subject on a large document, and a
  cancellation.
- `&`: members of differing widths, a member exceeding the measurement bound
  (truncation disclosed), a member containing a tab or CJK text (display width,
  not bytes).
- `%`: an empty document, a single-line document, a document with no trailing
  newline.
- `C` / `Alt-C`: growth past the last line (no-op + report), growth past the 256
  ceiling (refuse), and that the set stays sorted with the primary intact.
- `(` / `)` vs `Alt-(` / `Alt-)`: rotating focus must **not** move bytes; rotating
  contents must move bytes and must **not** change which member is primary.
- `Alt-minus` vs `Alt-_`: merge-all must bridge gaps; merge-consecutive must
  leave them.
- Insert-mode: all eighteen glyphs literal, all eight `ALT` chords inert.
- The key-authority guard reports **66** Normal-mode chords (48 + 18), two
  guard-block chords, one Insert chord, and no shadowed arm.
- M004's, M005's, M006's and M007's tests pass unmodified.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib tui::helix_selection
cargo test -p codegg --lib tui::commands::editor
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::interactive_terminal
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh quick
```

`verify.sh full` is required before closure. Never `--all-features`.

## 12. Documentation updates

- `docs/tui.md` — all eighteen verbs in the keymap, with `regex` prompts
  documented as prompts, and the Insert-mode inertness of the `ALT` chords
  stated.
- `architecture/tui.md` — the set-operation catalogue, the rotate-focus vs
  rotate-contents distinction, the `&` display-width decision, and the regex
  boundedness rule.
- Terminal-panel parity: record whether the panel gains any of these verbs.
  M007 preserved parity by construction; **eighteen new Normal-mode chords are
  the first thing in this roadmap that could break it**, because the panel has
  its own Normal-mode table. A recorded exemption is acceptable; silence is not.
- `EditorCommandName::ALL` gains three names if §6 option 1 is taken; the
  palette and its docs must agree.

## 13. Acceptance criteria (externally observable)

1. All eighteen verbs are reachable through the real dispatch path and are
   correct for heterogeneous selection sets.
2. `S`, `K`, and `Alt-K` accept a regex, handle a no-match set without
   corrupting the buffer, report an invalid pattern, and are bounded.
3. `&` aligns in **display** columns, reusing `bounded_display_width`, and
   discloses truncation.
4. `%` selects the entire file, including for an empty and an unterminated
   document.
5. `(` / `)` move focus; `Alt-(` / `Alt-)` move contents. Confusing them
   corrupts either the cursor or the buffer, so each is separately tested.
6. `Alt-minus` bridges gaps and `Alt-_` does not.
7. Every mutating verb lands as **one** transaction, and the 256 ceiling refuses
   rather than truncates.
8. A no-op reports and pushes no undo entry.
9. No new glyph is swallowed in Insert, and all eight `ALT` chords are inert
   there.
10. `check_tui_editor_key_authority.py` reports 66 Normal-mode chords and no
    shadowed arm; the other two editor guards pass; `verify.sh quick` passes.
11. The terminal-panel parity decision is recorded.

## 14. Stop conditions

Report rather than improvise if:

- M007 closed without exposing the §5 contract. That is a shape gap in M007,
  and the fix is a corrective pass against it — not a private type redefined
  here, which is how the two surfaces drifted the first time.
- The regex prompt cannot avoid adding a `TuiDocumentPresentation` field whose
  name contains `prefix`, **and** the only way forward is to weaken
  `scripts/check_tui_editor_prefix_state.py`. Report — that guard's invariant is
  M002's central claim.
- `Alt-` chords cannot be encoded as `Char` + `ALT`. Unverified: whether a given
  terminal delivers `Alt-(` as `Char('(')` + `ALT` or as a distinct key code is
  terminal-dependent, and `tests/modifier_chords.rs:42-52` records a related
  legacy-encoding footgun for `Ctrl`. If the encoding is not `Char` + `ALT`,
  the guard's parser needs teaching the new shape — report rather than adding a
  bypass.
- Any verb needs a filesystem read, a process, or a shell pipe. Helix's shell
  filters are explicitly deferred by the roadmap; `S` and `K` match in-buffer
  text only.
- `&` turns out to need byte alignment rather than display width to be useful,
  or the truncation disclosure cannot be made visible. Report rather than ship a
  silently wrong alignment.

## 15. Closure evidence required

Implementation commits; §11 output labelled local vs CI; explicit confirmation
that M004's, M005's, M006's and M007's tests passed **unmodified**; the
key-authority guard's chord counts before and after; a non-vacuity demonstration
for the Insert-mode sweep (failing on an injected claim) and for the key-authority
guard (failing on an injected shadow); the terminal-panel parity decision; and
the §6 prompt decision with its rationale.

## 16. Handoff notes

Three things a reviewer should look at first.

**Why the plan exists while blocked.** §5 is the reason. M007 defines a set API
that M008 is written against, and two plans written months apart against an
imagined interface is exactly how the `leaf()` → `leaves()` correction happened.
Freezing §5 during M007's review is the highest-value thing this document does.

**Whether eighteen chords is one milestone.** `plans/003-planning-process.md` §5
says a milestone is too large when it combines several independently releasable
capability boundaries. This one is arguably three: the **selection-manipulation**
verbs (`;`, `Alt-;`, `Alt-:`, `,`, `Alt-,`, `(`, `)`, `Alt-minus`, `Alt-_` —
pure set algebra, no edits, no new dependencies), the **ad-hoc set-construction**
verbs (`C`, `Alt-C`, `%` — cursor placement), and the **content-mutating** verbs
(`S`, `K`, `Alt-K`, `&`, `Alt-(`, `Alt-)` — regex, prompts, alignment). If the
review prefers, splitting along those three lines is straightforward and would
let the set algebra land first and be proven before regex work begins.

**The `regex` finding is the one that changes the sizing estimate.** `S`, `K`,
and `Alt-K` were the expensive-looking half of this milestone on the assumption
they'd need a new dependency. They do not — `regex` is already a root-package
dependency already used under `src/tui/`. What remains expensive there is the
**prompt** and the boundedness work, not the matching. That is why §6 recommends
reusing `:` rather than building a new input mode: the prompt is the whole cost.
