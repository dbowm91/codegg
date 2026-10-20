# TUI editor Helix parity Milestone 005 — Textual motions and goto mode

Status: ready for handoff
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M005
Long-term requirements: `plans/000-long-term-specification.md` (TUI editor);
`plans/002-long-term-roadmap.md` (editor capability sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 1. Objective

Land the three textual motions that make line-oriented navigation usable, and
open goto mode beyond its single `gg` leaf:

- `gs` — jump to first non-whitespace on the current line.
- `W` / `B` / `E` — long-word motions (whitespace-delimited runs).
- `gj` / `gk` — line down / line up, preserving the desired column.
- Purely **local** goto leaves (`ge`, `gh`/`gl`, `gj`/`gk`, `gs`) so the `g`
  prefix is a real mode rather than a single binding.

`gs` is the highest-value single key here: today there is **no** way to reach
first non-whitespace — `0` is the only line-start key — so indentation-heavy
code forces every cursor placement through a bare column 0.

> **Corrected against the live keymap during implementation (2026-10-18).**
> `https://docs.helix-editor.com/keymap.html` was re-read at the keystroke and
> two leaves in the first draft of this plan were wrong:
>
> - **`gl` is `goto_line_end`, not "last goto".** Helix's goto mode binds `gh`
>   to line start and `gl` to line end. This plan originally wanted a one-slot
>   `gl`; there is no jumplist here to repeat, so that was both wrong about
>   Helix and would have been a dead key. The real `gh`/`gl` are implementable
>   and shipped.
> - **`gt` is `goto_window_top`, not "last line".** Together with `gc`
>   (`goto_window_center`) and `gb` (`goto_window_bottom`) these are
>   **screen**-relative. This editor resolves motions against
>   `NOMINAL_VISIBLE_LINES`, a fixed guess rather than the real viewport, so
>   all three are **deferred**: binding them today would land the cursor on a
>   line that is not the top/middle/bottom of the reader's screen.
> - `gj`/`gk` are Helix's *textual* line motions against `j`/`k`'s *visual*
>   ones — not aliases. With no soft-wrap display-line model the two coincide
>   here, so the leaves resolve to the same commands rather than to
>   near-duplicates that could drift.

## 2. Why this milestone is ready

- **M002 — closed** (`plans/closure/tui-editor-helix-parity/002-status.md`).
  Goto mode is a real `EditorPrefix` variant with `arm_prefix` /
  `resolve_pending_prefix`.
- **M003 — closed**. Character-finding capture established the rule for a
  prefix whose next key is data; not needed here, but the grammar is proven.
- No ADR gates this scope. Every leaf in §1 is a pure cursor move over the
  frontend document replica.

Interface dependency: the goto leaf table proposed in §6. **This is a change to
an existing public shape** (`EditorPrefix::leaf`), and it is shared with M010
(space mode). It is sequenced here because M005 is the first milestone to need
more than one leaf, and getting the shape right once is cheaper than carrying a
single-leaf assumption into both.

## 3. Current implementation evidence

### `gs` does not exist; the helpers it needs largely do

`src/tui/editor.rs` already provides:

- `line_start(snapshot, line) -> Option<usize>`
- `line_content_end(snapshot, line) -> Option<usize>` — documented as
  inspecting **at most the last two bytes** so a multi-megabyte line costs a
  constant (C002).
- `buffer_content_end(snapshot) -> usize`
- `cursor_line(snapshot, offset) -> usize`
- `last_line_of(snapshot) -> usize`

`line_content_end` is the model for how a line-scan helper should be written
here: bounded, and covered by a test that would catch a whole-buffer scan.
`gs` needs a first-non-whitespace offset that is similarly bounded — on a
multi-megabyte indented line it must not scan the entire line.

### `word_forward` / `word_backward` / `word_end` exist but are byte-classified

These three functions back Helix's `w`/`b`/`e`. They classify with
`is_word_byte`, defined as `byte.is_ascii_alphanumeric() || byte == b'_' ||
byte >= 0x80`.

The `>= 0x80` clause means **any non-ASCII byte counts as word content**. The
existing doc comments on `word_end` state this is deliberate: the byte-at-a-time
scan can cross the interior of a multi-byte character and still classify it
correctly.

Consequence for this milestone: `W`/`B`/`E` must be defined in terms of
**whitespace-delimited runs**, not `is_word_byte` classes. Treating every
non-ASCII byte as word content makes `W` and `w` coincide on any document with
punctuation or CJK text, which would make `W` a binding with no observable
effect on a meaningful fraction of real files. §6 specifies the classification
explicitly.

### No soft wrap, so `gj`/`gk` are buffer-line motions

`src/tui/wrap.rs` provides word-wrap helpers used by TUI text surfaces, but the
editor's motion path works in **buffer lines** (`editor_motion` takes
`visible_lines` for viewport purposes only; `line_start` / `cursor_line` index
by logical line). There is no soft-wrapped display-line model in the editor
today.

Therefore `gj`/`gk` move by **logical line** and preserve a desired column.
This is the correct Helix behaviour for an unwrapped buffer and matches what
`Ctrl+d` / `Ctrl+u` already do. Making `gj`/`gk` display-line-aware would
require introducing a wrapped display-line model into the motion path — a
separate change, explicitly out of scope here.

### `leaf()` returns a single glyph — the shape that must change

`EditorPrefix::leaf(self) -> Option<char>` currently maps:

- `Goto` → `Some('g')`
- `Space` → `Some('y')`
- everything else → `None`

The doc comment states: *"Every leaf is a bare unmodified glyph today, so the
dispatch layer is the only place that needs to know about modifiers. The find
prefixes are `None` on purpose: their next key is a target character, not a
leaf."*

This works for `gg` (one leaf) and `space y` (one leaf). M005 needs **several**
goto leaves (`gs`, `ge`, `gt`, `gl`/`gk`), and M010 will need ~15 space-mode
leaves including modified glyphs. A single `Option<char>` cannot express that
set without becoming a special case per prefix.

`scripts/check_tui_editor_prefix_state.py` asserts the presence of
`pub enum EditorPrefix` with `Idle`, `pub pending_prefix`,
`MAX_EDITOR_PENDING_PREFIX`, and that `arm_prefix` / `disarm_prefix` /
`resolve_pending_prefix` exist. It does **not** pin `leaf()`'s signature, so
the shape change is possible without weakening the guard — but the guard's
model documentation should be updated to describe the new leaf table.

### `[` / `]` are armed with no leaves

Per the M002 closure, `OpenBracket` / `CloseBracket` exist with no matching
second key, and an unmatched second key disarms and resolves normally (proven
by a test asserting the cursor moved, not merely that the key was returned).
M006 will add those leaves. This milestone must not disturb that behaviour.

## 4. Invariants that must not regress

1. **Unclaimed-key fallthrough.** In Normal mode, any key the buffer does not
   own returns `BufferKey::Unhandled`. `g` is already claimed, so this is about
   not regressing the *fallthrough* for unclaimed second keys.
2. **Prefix mismatch semantics.** An unmatched second key after a goto prefix
   must keep disarming and resolving normally, matching M002's established
   behaviour. `g` followed by an unbound leaf must not swallow the key.
3. **Insert-mode text integrity.** `g`, `G`, `W`, `B`, `E` must remain literal
   text in Insert. Shifted letters are already composed text there (M001
   covered this), so `W`/`B`/`E` must not be claimed in Insert.
4. **Char boundary safety.** `gs` and the long-word motions must land only on
   `char` boundaries. The M003 defect — `read_bytes` refuses ranges not aligned
   on both ends, making the leading byte of a multi-byte character unreadable
   — is the precedent here; do not reintroduce it.
5. **Bounded line scans.** `gs` must not scan an entire line. Follow the
   `line_content_end` precedent: inspect a bounded prefix and have a test that
   would fail on an unbounded scan.
6. **Shared prefix machinery.** New leaves must be expressed through
   `resolve_pending_prefix`, not a parallel mechanism. M002's entire point was
   deleting the two legacy mechanisms; a third would regress it.
7. **Terminal-panel parity.** `src/tui/commands/interactive_terminal.rs` speaks a
   deliberate subset of the same dialect. The roadmap forbids letting the two
   diverge; §12 requires a parity decision and test for anything this milestone
   changes that the panel shares.

## 5. Scope

### In scope

- `gs` first non-whitespace.
- `W` / `B` / `E` long-word motions, whitespace-delimited.
- `gj` / `gk` line down / up, as goto leaves sharing `j`/`k`'s resolved motion.
- Goto leaves: `gs`, `ge` (end of file), `gh` (line start), `gl` (line end),
  plus the existing `gg`.
- The goto leaf table shape change (§6).
- Tests, guard updates, documentation updates.

### Out of scope

- **All LSP-backed goto leaves** — `gd`, `gy`, `gi`, `gr`. These belong to
  M010 and are additionally gated on M013, which repairs two blocking LSP
  delivery defects (see §6). Landing them here would bind keys to an LSP path
  that is not yet reliable.
- **Screen-relative goto leaves** — `gt` (window top), `gc` (window center),
  `gb` (window bottom). They need the real viewport height, and motions
  currently resolve against `NOMINAL_VISIBLE_LINES`, a fixed guess. Recorded
  omission, not silence: see the correction note in §1.
- `gw` label navigation — no anchor/scroll-offset model exists.
- `g|` (goto column) — needs a count entry point, as Helix's `G <n>` does.
  This editor's `G` takes no count.
- Buffer / alternate-file leaves (`gf`, `ga`, `gm`, `gn`, `gp`, `g.`) —
  multi-document attachment is a predecessor non-goal of this roadmap.
- Jumplist (`Ctrl-o` / `Ctrl-i`).
- Syntax-aware motions (ADR-0034, proposed).
- `[` / `]` leaves — M006.
- Anything in M004; the two milestones touch disjoint surfaces.

## 6. Required production changes

### The goto leaf table (the structural change)

Replace the single-glyph assumption with an explicit leaf set per prefix.
Preferred shape: keep `display()` as the status-row affordance (the label shown
while armed — the existing `gg` / `space y` tests report it, so they stay
unmodified) and replace the single-glyph `leaf()` with a **leaf table** mapping a
prefix to its accepted second keys and the action each one runs. This preserves
the existing rendering contract while allowing N leaves.

Concretely:

- Add `PrefixLeaf { glyph, action }` and `PrefixAction::{Motion(EditorCommand),
  YankSelection}`. The action is an enum, not a bare `EditorCommand`, because
  `space y` is not a motion — it reports a byte range for the App layer to copy.
- `EditorPrefix::leaves(self) -> &'static [PrefixLeaf]`.
- `Goto` accepts `g`, `s`, `e`, `h`, `l`, `j`, `k`.
- `Space` keeps `y` and gains nothing in this milestone (M010 owns its leaves).
- Find prefixes stay empty/data-capture, unchanged.
- Update `scripts/check_tui_editor_prefix_state.py`'s model documentation to
  describe the leaf table, and keep its existing assertions intact — adding
  `pub fn leaves(` to its required-declaration list rather than relaxing any.

**Rationale for doing this now rather than in M010:** M010 is a larger milestone
(space mode plus pickers plus LSP mapping). Doing the shape change inside M005
keeps each milestone to one ownership boundary and means the M010 plan inherits
a proven table.

### Textual motions (`src/tui/editor.rs`)

- `first_non_whitespace(snapshot, offset) -> usize` — bounded scan forward from
  the cursor's line start to the first non-whitespace byte, or
  `line_content_end` if the line is entirely whitespace. Returns the line start
  for a blank line so `gs` round-trips with `0`.
- `long_word_forward` / `long_word_backward` / `long_word_end` — whitespace-
  delimited runs.

The long-word classification must be defined explicitly as **whitespace
versus non-whitespace**, not by reusing `is_word_byte`. `is_word_byte`'s
`>= 0x80` clause exists to make the byte-at-a-time `w`/`b`/`e` scan correct for
multi-byte characters; reusing it for `W`/`B`/`E` would make them collapse onto
`w`/`b`/`e` wherever punctuation or non-ASCII text is present.

Reuse `next_char_boundary` / `prev_char_boundary` for stepping, and
`byte_at`'s `unwrap_or(0)` convention only where a byte is genuinely
sufficient.

### `gj` / `gk`

- Implement as logical-line down/up via the existing `line_start` /
  `cursor_line` helpers, carrying a **desired column** in
  `TuiDocumentPresentation` so moving down a short line and back up returns to
  the original column.
- The desired column must be updated by horizontal motions and by `|`-style
  placement if one is added later; it must be **cleared** when the cursor is
  placed by a non-column motion (`G`, search, `w`, `e`) exactly as vi-family
  editors do.
- Adding a presentation field requires updating
  `scripts/check_tui_editor_prefix_state.py` only if its
  `TuiDocumentPresentation`-declares-a-second-prefix-field assertion could be
  confused by it. **Read that assertion before adding the field** — the guard
  distinguishes prefix fields, and the new field is not one.

### Dispatch (`src/tui/commands/editor.rs`)

- Add `gs` to the goto leaf table; add Normal-mode arms for `W`, `B`, `E`, `gj`,
  `gk`.
- `G` and `g` remain as they are.

### Documentation

- `docs/tui.md` — editor keymap gains the new motions.
- `architecture/tui.md` — the leaf-table shape, and the `gt` = last line
  (not last tab) divergence.

## 7. Ordered work packages

**W1 — Leaf table shape change.** Introduce the goto leaf table, prove `gg` and
`space y` are unchanged by running their existing tests **unmodified** (M002
established that `pending()` reports `display()`, so the four existing glyph
assertions should need no rewrite). This is the acceptance signal: if those
tests need editing, the shape change leaked into behaviour.
Acceptance: M002 and M003 prefix tests green **without modification**.

**W2 — `gs` + bounded scan test.** `first_non_whitespace` with a multi-megabyte
indented-line test mirroring `line_content_end_does_not_read_a_huge_line`.
Acceptance: `cargo test -p codegg --lib tui::editor` green.

**W3 — Long-word motions.** `W`/`B`/`E` plus tests proving they differ from
`w`/`b`/`e` on punctuation-heavy and non-ASCII text, and land on char
boundaries.
Acceptance: focused unit tests green.

**W4 — `gj`/`gk` and desired column.** Field, motion arms, round-trip test
(down a short line, back up, column restored), and a clear-on-motion test.
Acceptance: focused tests green; prefix-state guard still passes.

**W5 — Guards and docs.** §6 guards, `docs/tui.md`, `architecture/tui.md`.
Acceptance: both guard scripts exit 0.

**W6 — Full verification.** §11.

### 7.1 Outcome recorded after execution (2026-10-18)

W1–W6 all landed. Two **pre-existing behaviours changed** as a result, both
deliberately: `j`/`k` now keep a desired column (an existing test asserted the
opposite on purpose), and `w`/`b`/`e` now work on non-ASCII text (a latent
defect). Both are documented with their measured before/after in **§16**.

`gg` and `space y` behaviour tests passed unmodified. The one test that changed
asserts the *old accessor shape* (`EditorPrefix::leaf`), which is exactly what
W1 replaces.

## 8. Failure, cancellation, restart, contention semantics

These verbs do not mutate text, so the failure surface is small — but two cases
are real:

- **`gs` on a whitespace-only or empty line** must land on the line start, so
  it round-trips with `0` rather than appearing stuck.
- **Desired column across a document reload / buffer switch** must be reset.
  A column computed against the old document is meaningless; leaving it set
  would make the next `j` land somewhere unrelated.

Neither is a cancellation case; no verb here is cancellable mid-motion.

## 9. Compatibility and migration

None. No protocol, storage, or configuration change. The one shape change
(`leaf()`) is internal to the editor module and the guard that covers it.

## 10. Required tests

- M002/M003 prefix tests **unmodified** after W1 (the regression signal).
- `gs` on: blank line, indented line, line starting mid-word, whitespace-only
  line, single-line document, multi-byte leading character.
- `gs` bounded scan: multi-megabyte indented line, mirroring the existing
  `line_content_end_does_not_read_a_huge_line`.
- `W`/`B`/`E` vs `w`/`b`/`e` differentiation on punctuation and non-ASCII text.
- `W`/`B`/`E` char-boundary landing with CJK/accented text.
- `gj`/`gk` desired-column round trip; desired column cleared by `G`, search,
  `w`, `e`.
- Insert-mode integrity for `g`, `G`, `W`, `B`, `E`.
- Prefix mismatch: an unbound goto second key disarms and resolves normally —
  assert the cursor **moved**, per M002's convention.
- `[`/`]` with no leaves must be unchanged.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::commands::editor
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_key_authority.py
scripts/verify.sh quick
```

`verify.sh full` is required before closure.

## 12. Documentation updates

- `docs/tui.md` — new motions in the editor keymap.
- `architecture/tui.md` — the leaf-table shape after W1; the `gt` divergence.
- Terminal-panel parity: determine whether
  `src/tui/commands/interactive_terminal.rs` gains or explicitly declines these
  motions, and record which. The roadmap forbids silent divergence; a recorded
  exemption is acceptable, silence is not.

## 13. Acceptance criteria (externally observable)

1. `gs` reaches first non-whitespace on any line and lands on the line start
   for a blank line.
2. `W`, `B`, `E` move by whitespace-delimited runs and are observably
   different from `w`, `b`, `e` on punctuation-bearing text.
3. `gj`/`gk` move by logical line and preserve the desired column across a
   short-line round trip.
4. The `g` prefix accepts `gs`, `ge`, `gh`, `gl` as well as `gg`; an unbound
   second key still disarms and resolves normally. (`gt`/`gc`/`gb` are
   deliberately unbound — §5.)
5. `gg` and `space y` behave exactly as before (existing tests unmodified).
6. `[` / `]` with no leaves behave exactly as before.
7. None of the new keys are captured in Insert mode.

## 14. Stop conditions

Report rather than improvise if:

- The leaf-table shape change would require weakening
  `scripts/check_tui_editor_prefix_state.py` rather than updating its model
  documentation.
- Preserving `gg`/`space y` behaviour requires editing their existing tests —
  that means the shape change altered behaviour, and must be resolved before
  proceeding.
- The desired-column field conflicts with the guard's
  `TuiDocumentPresentation` assertion (§6).
- `gj`/`gk` turn out to require a wrapped display-line model to be correct for
  this editor's rendering — report, since that changes the milestone's meaning.

## 15. Closure evidence required

Implementation commits; §11 output labelled local vs CI; explicit confirmation
that M002's and M003's prefix tests passed **unmodified**; the terminal-panel
parity decision from §12; and a guard demonstration that
`check_tui_editor_prefix_state.py` fails on an injected violation.

## 16. Two pre-existing behaviours changed, on purpose

Recorded here because both are invisible until someone runs `j` twice or opens a
CJK file.

### 16.1 `j`/`k` now keep a desired column

`vertical_motion_clamps_to_shorter_target_lines` asserted the opposite on
purpose — its comment read "Column 2 is preserved relative to the line start,
not to byte 10", i.e. `j` down onto a short line and `k` back returned to the
**short line's** clamped column. That is precisely the deficiency §6 set out to
remove, and exactly what Helix's `desired_col` exists to prevent. The test was
rewritten to the new contract and renamed
`vertical_motion_keeps_the_column_across_a_shorter_line_and_resets_after_a_jump`,
which also asserts the run is not sticky.

Because `gj`/`gk` are the *same* commands as `j`/`k` in an editor with no
soft-wrap (§1), they share one column rather than starting a second, disagreeing
run. Giving only `gj`/`gk` the column was rejected: it would make two keys that
Helix defines as one behave differently, and would leave `j` still losing the
reader's place.

### 16.2 `w`/`b`/`e` now work on non-ASCII text

Found while implementing W3, and **not optional**. `byte_at(offset)` was
`read_bytes(offset, offset + 1)`, and `read_bytes` refuses a range that is not
boundary-aligned at *both* ends — so it returns `Err` for the first byte of a
multi-byte character as well as its interior. Every word scanner read that as
"not a word byte" and skipped forward.

Measured on `日本語 text` before the fix:

| From | `w` lands on | correct | `e` lands on | correct |
|---|---|---|---|---|
| byte 0 (日) | 14 — end of document | 10 | 14 — end of document | 6 |
| byte 10 (`t`) | 14 | 14 | 14 | 13 |

This is the defect class M003 eliminated in `find_char`
(`plans/closure/tui-editor-helix-parity/003-status.md`) — the word scanners were
never given the fix — and it was invisible because ASCII single-byte characters
are the only kind that reads back, so the surface had no non-ASCII test at all.
A `word_end` doc comment asserted the opposite of the truth, claiming that
walking `offset + 1` "can cross the interior of a multi-byte character and still
classify it correctly". Shipping `W`/`B`/`E` on top of that primitive would have
left half the word-motion family skipping entire CJK words, and M005's own
acceptance criterion — that `W`/`B`/`E` are *observably distinct* from
`w`/`b`/`e` — would have been meaningless.

Fixed by classifying with `leading_byte`, which reads the character's own byte
range. Verified in a scratch crate against `codegg-document` **before** paying
for a workspace build: ASCII results are byte-identical to the pre-fix behaviour
across every boundary offset of `foo.bar baz`, `éx yó` and `日本語 text`.

### 16.4 A goto leaf and a held selection

The plan assumed goto leaves "collapse the selection", as Helix's goto mode
does. They do not, and did not for `gg` either: `apply_motion` never touches
`selection`, so a leaf leaves a held selection exactly where `gg` left it. The
first version of the dispatch test asserted the collapse and failed.

`apply_motion` is shared with the bare `G`, `w` and `b`, so making it clear the
selection is a change to existing behaviour well outside this milestone.
Recorded as a **known divergence from Helix** rather than absorbed. What M005
does own is parity between the old leaf and the new ones, and
`every_goto_leaf_leaves_a_held_selection_exactly_as_gg_does` pins that by
comparing every leaf against a `gg` baseline established in the same test.

### 16.5 `long_word_end` could not terminate

The same investigation turned up an unbounded loop.
`next_char_boundary` returns `None` — not `Some(len_bytes())` — when the
remaining bytes are one multi-byte character, so on any document ending in a
multi-byte character the `cursor = next_char_boundary(..).unwrap_or(cursor)`
step had nowhere to advance and the loop condition never changed. The existing
`word_end` had the identical shape but escaped by luck: `is_word_byte(0)` is
`false` (NUL is not alphanumeric), so an unreadable byte stopped it, whereas the
inverted `is_long_word_byte(0)` is `true`.

Found by sampling the running test binary's stack after a 14-minute hang, not by
reading the code. The reasoning that it must terminate was wrong: `len_bytes() - 1`
is the last byte of the final character, so it is **not** a character boundary
for any multi-byte character. Fixed by whole-character stepping in both
`*_end` scanners; a regression test sweeps every boundary offset of five
documents, three of which end in a multi-byte character.

## 16. Handoff notes

Do W1 first and run the M002/M003 tests before writing anything else. If those
pass unmodified, the structural risk in this milestone is retired and the rest
is ordinary motion work. If they need edits, stop and report per §14.

For LSP-backed goto leaves (`gd`, `gy`, `gi`, `gr`), do **not** start them here
— see the M013 dependency in §5.

The nearest precedent to copy structure from is
`plans/implementation/tui-editor-helix-parity/002-unified-prefix-state-machine.md`,
which owns the prefix grammar this milestone extends.