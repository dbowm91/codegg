# TUI editor Helix parity Milestone 009 — Match mode and text objects

Status: active
Repository baseline: `a8778e87c` (`feat/auto-20261018-12255c45`); W1 was
implemented against the same baseline (uncommitted at the time of writing)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M009
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`;
`plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` (**proposed** — see §2)
Primary class: capability

> **Note on scope.** This plan delivers M009's **byte-derived** boundary:
> surround add/replace/delete, the word / WORD / paragraph / delimiter-pair
> text objects, and pair matching. The **syntax-node** object slice
> (`f`, `t`, `a`, `c`, `T`, `x`) is **not** here. ADR-0034's own decision text
> says that slice is gated on the ADR's acceptance, and Helix's text-object
> table says the same thing independently. The roadmap's M009 dependency line
> is amended accordingly (Amendment **A3**), and the gated slice is registered
> as its own milestone so that it is not an orphan. See §2 and §5.

## 1. Objective

Give the editor Helix's `m` grammar.

Today the editor has **no** match mode, **no** surround, and **no** text
objects. `m` is unclaimed in the editor route and falls through to the
composer. This milestone adds:

- `m m` — jump to the matching bracket of the pair under the cursor;
- `m s <char>` — surround the held selection with `<char><char>`;
- `m r <from><to>` — replace the surrounding `<from>` pair with `<to><to>`;
- `m d <char>` — delete the surrounding `<char>` pair, keeping the contents;
- `m a <object>` — select **around** the object;
- `m i <object>` — select **inside** the object;

where `<object>` is one of the byte-derived objects Helix supports without a
parser: `w` (word), `W` (WORD), `p` (paragraph), a delimiter-pair character
(`(`, `[`, `{`, `<`, `'`, `"`, …), or `m` (the closest enclosing pair).

The deliverable is **structural editing without a syntax tree**. That is a
real capability, not a stub: the pair table and the scanners are ordinary
byte work, and Helix's own documentation describes exactly this subset as
parser-free.

## 2. Why this milestone is ready

- **The dependency that blocked it has been discharged by the ADR's own
  text.** `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` records, in
  its Decision section: *"Word and delimiter-pair text objects are implemented
  frontend-side from bytes and are explicitly **not** gated on this decision —
  they do not need a parser."* Its Option C is written as *"Land M001–M008,
  M010–M012 and their non-TS slices"*, and the Context section says the same:
  *"Text objects and surround (`m a`/`m i`, `m s`/`m r`/`m d`) are partially
  reachable without a parser — word and delimiter-pair objects are computable
  from bytes — but block objects and syntax-node-aware selection need one."*
  The registry's own M009 cell already says the same thing: *"Word and
  delimiter-pair text objects do **not** need the ADR and must not be gated
  behind it."*
- **Helix's text-object table agrees, independently of any ADR.** The Helix
  document lists `w`, `W`, `p`, the specified surround pairs and `m` as
  parser-free, and marks `f`, `t`, `a`, `c`, `T` and `x` as needing a
  tree-sitter grammar. The split this plan draws is the one Helix draws.
- **The M008 edge is closed.** `plans/closure/tui-editor-helix-parity/008-status.md`
  closed the multi-selection command surface, which is what supplies the
  selection-set primitives this milestone builds on.
- **No new dependency, no protocol change, no daemon change.** Everything
  here is frontend byte work over a snapshot the editor already holds.

## 3. Current implementation evidence

All claims below were verified by reading the tree at `a8778e87c` with the
M013 work applied. Line numbers are for that tree.

### 3.1 Nothing exists yet

- `grep -i 'surround|text_object|matching_bracket|delimiter_pair|pair_table|bracket_pair' src/`
  returns 16 hits, **every one of them English prose** in unrelated modules
  (`src/tool/bash/policy.rs:360`, `src/tui/app/render.rs:2699`, …). There is
  **no code**.
- `src/tui/helix_selection.rs` exists but is **not** bracket matching. It is
  pure set algebra over points: `Selection<P>`, `SelectionState<P>`, and the
  rules `toggle_char` / `select_char` / `select_line` / `flip_all` /
  `ensure_forward` / `keep_primary` / `remove_primary` / `rotate_primary` /
  `merge_all` / `merge_consecutive`. It has **no** notion of a bracket, a
  pair, a delimiter, or a text object.
- `m` is unclaimed: `handle_normal_key` (`src/tui/commands/editor.rs:3138`)
  has no `m` arm and ends with `_ => BufferKey::Unhandled` (`:3653`).

### 3.2 The prefix machine is **one prefix, one terminal key**

This is the milestone's central constraint, and it is a *documented* one.

- `EditorPrefix` (`src/tui/editor.rs:98-124`) has **ten** variants: `Idle`,
  `Goto`, `Space`, `OpenBracket`, `CloseBracket`, `FindNext`, `FindPrevious`,
  `FindTillNext`, `FindTillPrevious`, `ReplaceChar`.
- `EditorPrefix::leaves(self) -> &'static [PrefixLeaf]` (`:241`) maps a prefix
  to a flat glyph→action table. There is no second level.
- The prefix state is **one field**: `TuiDocumentPresentation.pending_prefix`
  (`src/tui/document_session.rs:140`), armed by `arm_prefix` (`:248`), which
  **replaces** rather than stacks. A test pins that:
  `arming_replaces_rather_than_stacking` (`src/tui/editor.rs:6663`).
- `scripts/check_tui_editor_prefix_state.py:251-266` fails if
  `TuiDocumentPresentation` declares more than one field whose name contains
  `prefix`.
- The design note at `src/tui/editor.rs:87-91` states the rule: a grammar
  arrives "as a variant here and its leaf in `commands::editor::resolve_pending_prefix`,
  never a new field on the presentation."

**Helix's `m` is a two-level grammar** — `m` + command, then command +
argument (`m s (`, `m r ([`, `m a w`). A single terminal key cannot express
it. §6 specifies the resolution.

### 3.3 A verb **can** replace the selection set

`SelectionState::set_members(cursor, selections, primary) -> bool`
(`src/tui/helix_selection.rs:570`) is the single constructor every set-building
verb uses, and it normalizes through `normalize_set` (`:42`). Precedents:
`HelixVerb::SelectAll` (`src/tui/editor.rs:2860-2868`) and
`HelixVerb::CopySelectionOnLine` (`:2880-2934`) both build a fresh member
list and call `set_members`. `m a` / `m i` are therefore implementable as
**"set the selection to an arbitrary range"**, not merely extend or toggle.

One coordinate rule matters: a character selection's range runs to
`char_end(head)`, so a member built to cover `start..end` must name the **last
character's start** as its head (`src/tui/editor.rs:3217-3221`;
`last_char_start` at `:3222`). Pointing `head` at `end` would cover one
character too many.

### 3.4 The verb plumbing

```
handle_editor_key(app, key)                 src/tui/commands/editor.rs:2036
└─ editor_buffer_key(state, mode, key)      :3051
   └─ handle_normal_key(state, key, vis)    :3138
      ├─ resolve_pending_prefix(...)        :2921
      │  └─ editor::apply_helix_verb(...)   src/tui/editor.rs:2374
      └─ match (key.modifiers, key.code)    :3160
         ├─ apply_helix(state, verb)        :3775
         └─ apply_helix_edit(state, verb)   :3804
            └─ submit_edit(state, edit)     :1496
               └─ controller.apply_local(..) + editor::push_undo(..)
```

- `PrefixAction` (`src/tui/editor.rs:289-315`) has four arms: `Motion`,
  `YankSelection`, `Edit`, `Lsp`. **There is no arm for a
  selection-shaping verb** — `Motion` is "the one function in the dispatch
  that promises never to touch text" (`:300-302`) and `Edit` takes an
  `EditorCommand`, which is a finite `Copy` enum of motions and deletions.
- `apply_helix` (`src/tui/commands/editor.rs:3775`) carries the **read-only
  gate**: `grows_selection` (`:3800`) matches only `SelectChar | SelectLine`,
  and `mutates_buffer` (`:3813`) matches only
  `AlignSelections | RotateSelectionContents`. **A new editing verb that is
  not added to `mutates_buffer` bypasses the read-only refusal.**
- `EditorUndoEntry.selections_before`
  (`src/tui/editor.rs:1320`) already carries the whole selection set, so
  surround edits get correct undo for free **if and only if** they route
  through `submit_edit`.

### 3.5 Reusable byte helpers

| Helper | Location | Note |
|---|---|---|
| `is_word_byte` | `src/tui/editor.rs:1901` | ASCII alnum, `_`, or `>= 0x80` — the classification `w`/`b`/`e` use |
| `is_long_word_byte` | `:1913` | "not whitespace" — the `W`/`B`/`E` classification |
| `leading_byte` | `:1930` | char-safe byte read; `read_bytes(o, o+1)` is `Err` for every byte of a multi-byte char |
| `word_forward` / `word_backward` / `word_end` | `:1939` / `:1963` / `:2001` | byte-classified word motions |
| `next_char_boundary` | `:1572` | `pub` |
| `bounded_prefix_end` | `:1639` | `pub`; at most `MAX_RENDER_LINE_BYTES` past `start`, clipped to a char boundary |
| `char_end` / `last_char_start` | `:3418` / `:3222` | private; the two ends of a character range |
| `MAX_SET_VERB_READ_BYTES` | `:2780` | the existing per-verb across-member read budget |
| `MAX_RENDER_LINE_BYTES` | `:571` | 32 KiB |
| `MAX_EDITOR_SEARCH_BYTES` | `:621` | 2 MiB |
| `MAX_EDITOR_PENDING_PREFIX` | `:70` | 5 — the indicator label bound |
| `regex` | `Cargo.toml:49,213` | available in the root package; `MAX_REGEX_PROGRAM_BYTES` = 1 MiB (`:2788`) |

**Regex is the wrong tool for a bracket matcher** — it cannot count nesting.
A matcher must be a hand-written depth counter over bytes, following
`word_forward`'s precedent of `leading_byte` + `next_char_boundary`, never
`read_bytes(o, o+1)`.

### 3.6 Where the scanner must live

`src/tui/helix_selection.rs` is **pure set algebra over points** with no byte
knowledge; `architecture/tui.md:1323-1329` states the split: *"Set algebra is
pure … Everything that needs to resolve 'which byte is the next line' … is
geometry or content, and stays on the surface."* A bracket scanner needs
bytes. **It belongs on the surface** — `src/tui/editor.rs`, or a new
byte-oriented sibling module — and **not** in `helix_selection.rs`, or M008's
architectural split is broken.

## 4. Invariants that must not regress

1. **One owner per chord.** `scripts/check_tui_editor_key_authority.py`
   enforces disjointness between the mode-independent guard block, the
   Normal-mode table and the Insert-mode table. `m` is claimed by neither, so
   the new arm is safe — but the arm's **shape** must parse under the
   guard's arm-shape parser, and the guard *fails loudly* (exit 2) on a shape
   it cannot resolve. Copy the existing `(m, KeyCode::Char('x')) if m == none`
   shape exactly.
2. **One prefix field.** `scripts/check_tui_editor_prefix_state.py` fails if
   `TuiDocumentPresentation` grows a second `*prefix*` field, and fails if any
   occurrence of `pending_command`, `yank_prefix_pending`,
   `MAX_EDITOR_PENDING_COMMAND`, `bounded_pending` or `push_pending` appears
   anywhere under `src/` — **including in tests**.
3. **A capture is reachable from `is_capture`.** `EditorPrefix::is_capture`
   (`:165`) is what keeps a capture grammar from resolving through the leaf
   lookup and swallowing a keystroke. Any new capture variant **must** be
   added there, or the `r` regression recurs.
4. **The read-only gate.** Every new verb that edits text joins
   `mutates_buffer`; every new verb that grows the selection joins
   `grows_selection`.
5. **The insert-mode sweep is byte-exact.** `printable_characters_reach_the_document_in_insert_mode`
   (`src/tui/commands/editor.rs:5541`) sweeps 26 letters × 3 encodings + 15
   punctuation and **asserts `inspected == 93`** (`:5609`). `m` is already a
   swept letter, so the count must stay **93** unless a punctuation glyph is
   added to a Normal-mode table, in which case the count moves with it.
6. **The guard's chord counts are reported, not asserted.** The key-authority
   guard prints "N guard-block chord(s), N Normal-mode chord(s), N
   Insert-mode chord(s)". The closure record must state the new numbers.
7. **The text-authority guard.** `scripts/check_tui_editor_text_authority.py`
   forbids a struct field named `text`/`content`/`lines`/`body`/`buffer`/
   `rope`/`source`/`document`, forbids an owned `DocumentSnapshot` or
   `DocumentBuffer`, and forbids any filesystem read in the editor path. A
   text-object scanner must not retain document text in a struct field.

## 5. Scope

### In scope

- `EditorPrefix::Match` — the `m` prefix, with a leaf table.
- `m m` — pair matching (jump to the mate of the pair under the cursor).
- `m s` / `m r` / `m d` — surround add / replace / delete.
- `m a` / `m i` — select around / select inside, for the objects `w`, `W`,
  `p`, a delimiter-pair character, and `m` (closest enclosing pair).
- A **delimiter-pair table** and the byte scanners it needs.
- The read-only gate, the undo funnel, the prefix-state guard, the
  key-authority guard.
- Dispatch-path tests for every bound chord, and the doc surfaces in §12.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| `m a` / `m i` for `f`, `t`, `a`, `c`, `T`, `x` | Helix marks these as tree-sitter-only; ADR-0034's decision text gates the syntax-node slice on its own acceptance | **M014** (new milestone; see §16) |
| `m a` / `m i` for `g` (Change object) | Needs changed-line data the TUI has no source for — the same missing source as `]g`/`[g`/`]G`/`[G` | Deferred unregistered product work, alongside the change-navigation quartet |
| `m m` as a **syntax-tree** bracket match | Helix's `m m` is tree-sitter-backed; CodeGG's `m m` is a pair-table match. A syntax-aware upgrade is the same M014 slice | **M014** |
| Counts ("use counts to act on outer pairs") | This editor has no count entry point; the `GOTO_LEAVES` doc already records that `g\|` needs one | A future count milestone |
| `m` in Insert mode | Helix's `m` is a Normal-mode grammar | — |

### The Helix divergence this milestone records

Helix's keymap marks `m m` as **TS**. CodeGG's `m m` will be a **pair-table**
match: it finds the pair whose delimiter is under the cursor by scanning
outward with a depth counter, and jumps to its mate. That is a **deliberate
local divergence** from Helix, made because the alternative is a key that does
nothing. It is recorded here, in `docs/tui.md`, in `architecture/tui.md` and
in the `CHANGELOG.md` entry — not left to be discovered.

## 6. Required production changes

### 6.1 The grammar: a two-level `m` in a one-field machine

The machine holds one `EditorPrefix` and one `leaves()` table. Helix's `m`
needs two levels. The resolution that keeps **one field**:

- `EditorPrefix::Match` — armed by `m`. Its leaves are the *commands*:
  `m` (match), `s` (surround add), `r` (surround replace), `d` (surround
  delete), `a` (around), `i` (inner).
- `EditorPrefix::MatchArg(MatchArg)` — a **capture** that takes the
  *argument* as data, exactly like `f`/`t`/`F`/`T`/`r` do. `MatchArg` is a
  `Copy` enum carried **in the enum variant**, so the presentation still has
  exactly one prefix field and the prefix-state guard stays green.
  - `SurroundAdd` — one character.
  - `SurroundDelete` — one character.
  - `SurroundReplaceFrom` — one character; on capture it arms
    `SurroundReplaceTo`, which takes the second character. This is why the
    variant carries a payload: `mr([` is two characters, and a single
    `EditorPrefix` value must be able to say which half is pending.
  - `AroundObject` — one character, restricted to the object set.
  - `InnerObject` — one character, restricted to the object set.

Because `arm_prefix` **replaces**, `m` → `s` → `EditorPrefix::MatchArg(SurroundAdd)`
is a legal transition with one field and no new state. The sub-state rides in
the enum; the indicator renders `"m s"`, which is 3 characters against a bound
of `MAX_EDITOR_PENDING_PREFIX = 5`.

`EditorPrefix::is_capture` must return `true` for `MatchArg(_)` — this is
**invariant 3** above, and the single most likely way to get this milestone
wrong.

`EditorPrefix::display()` (`:202`) gains `"m"` for `Match` and `"m s"`,
`"m r"`, `"m d"`, `"m a"`, `"m i"` for the argument states. The test
`every_pending_prefix_display_fits_the_indicator_bound`
(`src/tui/editor.rs:6631`) uses an **explicit list** of variants; the new
variants must be added to it, or the test will not see them at all.

### 6.2 `PrefixAction` needs a selection-shaping arm

`m a` / `m i` produce a **selection set**, which is neither a motion nor an
`EditorCommand`. `PrefixAction` (`src/tui/editor.rs:289-315`) gains an arm
carrying the verb.

Because `HelixVerb` (`src/tui/editor.rs:2171-2230`) is **not** `Copy` — the
regex verbs carry a `String` — an arm holding a whole `HelixVerb` would
strip `Copy` from `PrefixAction` and `PrefixLeaf`. Two ways out; pick one and
record it:

- **(a) Widen the enum.** `PrefixAction::Verb(HelixVerb)`, dropping `Copy`
  from `PrefixAction` and `PrefixLeaf` (keeping `Clone`). `PrefixLeaf` lives
  in `&'static [PrefixLeaf]` const arrays, which `Clone`-only types support.
  `EditorPrefix` itself stays `Copy` — it holds no `PrefixAction`.
- **(b) A dedicated `Copy` payload.** A new `Copy` enum (e.g.
  `ObjectVerb { kind: ObjectKind, around: bool }`) as the arm's payload, mapped
  to a `HelixVerb` variant in the resolver.

**(a) is preferred**: one verb vocabulary, and the mapping is a no-op. The
ripple is mechanical and must be listed in the closure record. It is
exactly this:

- `PrefixLeaf` stops being `Copy` (`src/tui/editor.rs:319`), so the leaf
  lookup at `src/tui/commands/editor.rs:2950` changes from `.copied()` to
  `.cloned()`;
- the leaf tables are `const` arrays, so **every future leaf payload must
  be const-constructible** — `String::from("x")` is `E0015` in a const
  item, so a leaf can never carry a runtime-built `String`;
- `HelixVerb` becomes reachable from the public `PrefixLeaf` type, which
  puts its `String`-bearing variants in the public surface.

**Option (b) avoids all three** at the cost of one mapping arm, and keeps
the M005-frozen `EditorPrefix::leaves() -> &'static [PrefixLeaf]`
interface unchanged. The decision is recorded here; §14's stop condition
fires on (a) and the implementer must state which option was taken and
list the ripple that option actually produced.

A new `HelixVerb` variant carries the object request:

```rust
HelixVerb::SelectObject { kind: ObjectKind, around: bool }
```

`ObjectKind` is a `Copy` enum: `Word`, `LongWord`, `Paragraph`, `Pair(char)`,
`ClosestPair`, and — reserved for the M014 slice, never constructed here —
the syntax-node kinds. A `HelixVerb` variant that is never constructed is a
**claim, not a hook**; §7 therefore does **not** add syntax-node kinds to
`ObjectKind` in this milestone.

### 6.3 The object resolvers

Each resolver answers: *given a snapshot and a cursor, what byte range is the
object?* Each must be **bounded**.

- **Word (`w`)** — the byte-classified word at the cursor, using
  `is_word_byte` (`:1901`) so CJK bytes count as word bytes, matching the
  `w`/`b`/`e` motions. The word must be able to span a line boundary the way
  Helix's does.
- **WORD (`W`)** — the whitespace-delimited run, using `is_long_word_byte`
  (`:1913`).
- **Paragraph (`p`)** — the paragraph under the cursor. The paragraph scanner
  already exists for `]p`/`[p`: `next_paragraph_start`
  (`src/tui/editor.rs`, the `ParagraphMotion` rule referenced from
  `EditorCommand::NextParagraph`, `:1368-1375`) and its backward twin.
  Reuse them; do not write a third paragraph scanner.
- **Pair (`(`)** — the content between the matching open and close
  delimiters. The scanner is a depth counter over bytes. **W1 delivered it**
  as `src/tui/editor_match.rs` — a dependency-free module whose scanner does
  its own UTF-8 character-boundary stepping, because the crate helpers
  `leading_byte` / `next_char_boundary` operate on `DocumentSnapshot` and
  would make the module depend on the editor's document type. See §6.4.
- **Closest pair (`m`)** — the innermost enclosing pair, found by scanning
  outward and keeping the first complete pair that encloses the cursor.

Two word definitions exist in the tree and they are **not** the same:
`is_word_byte` (`src/tui/editor.rs:1901`) treats `>= 0x80` as word bytes, and
`word_at`'s local `is_word` (`:964`) does not. **`m i w` uses `is_word_byte`**,
so the object agrees with the `w` motion the reader just used to get there.
Record that choice.

Every resolver takes a **bounded window**. The precedent is
`bounded_regex_window` (`:3207`) and the budget `MAX_SET_VERB_READ_BYTES`
(`:2780`). A resolver that scans the whole buffer violates the roadmap's exit
condition *"objects resolve against a bounded window and never scan the whole
buffer per keystroke"* and must be rejected at review.

**The word must not be a whole-buffer scan.** A word that can span lines is
bounded by the search budget (`MAX_EDITOR_SEARCH_BYTES`, 2 MiB), not by
"the entire document" without a cap.

### 6.4 The pair table

**W1 delivered the pair table in a new module, `src/tui/editor_match.rs`**
(declared in `src/tui/mod.rs`). The module is **dependency-free** — it imports
only `std::ops::Range` and operates on `&str` plus byte offsets. This is a
deliberate, recorded deviation from the "a section of `src/tui/editor.rs`"
option this section originally allowed, and it is what allowed W1 to land
concurrently with M010's W1 without touching a shared file.

**Why the deviation.** The crate helpers `leading_byte` / `next_char_boundary`
operate on `DocumentSnapshot`; importing them would make the module depend on
the editor's document type and would put the scanner behind the crate's build.
The module therefore does its own UTF-8 character-boundary stepping — forward
via `str::chars()`, backward via the continuation-byte rule (`0b10xxxxxx`) —
and the two agree on boundaries. The call sites that need snapshot-relative
offsets use the crate helpers; the scanner uses its own. The deviation is
recorded in the module's own top doc comment, so it is not rediscovered.

The module holds:

- the **pair table**: `(`↔`)`, `[`↔`]`, `{`↔`}`, `<`↔`>`, `'`↔`'`, `"`↔`"`,
  and the backtick. A pair's open and close may be the same character
  (quotes), which the scanner must handle: a quote pair is the next matching
  quote, not a nesting depth.
- the **nesting rule**: for a pair whose open and close differ, the scanner
  counts depth and matches the *outermost* enclosing occurrence of the same
  delimiter; for a quote pair, it takes the next unescaped occurrence.

**W1's public API** (`src/tui/editor_match.rs`): `Pair { open, close }` with
`Pair::quote`, `Pair::is_quote`, `Pair::content_range`, `Pair::full_range`;
`PAIRS: [Pair; 7]`; `pair_for`, `pair_mate`, `is_pair_char`; and the three
scans `pair_from_open`, `pair_from_close`, `enclosing_pair`. Every scan
returns `MatchOutcome::{Found { open, close }, Unbalanced, BudgetExhausted}` —
**three** outcomes, not two, so a caller can tell "there is no mate" from "I
was not allowed to look that far" and refuse differently for each. The budget
is a `max_bytes` argument, and the anchor delimiter is the caller's and is not
charged against it.

`surround replace preserves nesting` is the roadmap's exit condition. The
replacement therefore replaces the **outermost** enclosing pair of the given
delimiter, and the replaced text keeps the inner pairs intact:
`(a(b))` with `mr([` becomes `[a(b)]`, not `[a(b])`.

`surround add` on a held selection inserts the open delimiter before and the
close delimiter after, and **grows the selection** to cover the new
delimiters — Helix's `m s` selects the surrounded text.

`surround delete` removes the delimiters and keeps the contents, and the
selection collapses to the contents.

### 6.5 `m m` — match bracket

From the cursor, find the pair under it and jump to the mate.

- If the cursor is on an opening delimiter, jump to its match.
- If the cursor is on a closing delimiter, jump to its match.
- If the cursor is on neither, **refuse with a reader-facing reason** rather
  than doing nothing: "no bracket under the cursor". This is the M002 failure
  mode — a key that silently does nothing — and the M013 precedent
  (`EditorPendingEffect::Unavailable`) is the model.
- If the pair is **unbalanced** (no match found within the bounded window),
  refuse with a distinct reason: "no matching bracket". Unbalanced input is
  a normal state for a buffer being typed into, and the milestone's exit
  condition names it explicitly.
- Multi-byte input must not corrupt the scan: the scanner steps by character
  boundaries and never reads a single byte as a character, so it never produces
  the `read_bytes(o, o+1)` failure the crate's `leading_byte` exists to
  prevent. In W1's module this is the dependency-free stepping described in
  §6.4.

`m m` is a **motion**, so it belongs in `PrefixAction::Motion` — which means
`EditorCommand` (`src/tui/editor.rs:1344-1410`) needs a `MatchBracket`
variant, and the geometry lives on the surface next to the other motions.
`EditorCommand` is `Copy` and finite, and `resolve_edit` (`src/tui/editor.rs`)
ends in `_ => None`, so a new variant must be wired there or it will resolve
to no edit.

### 6.6 The read-only gate and the undo funnel

- `mutates_buffer` (`src/tui/commands/editor.rs:3813`) gains the three
  surround verbs. Without this, a surround on a read-only attachment would
  edit text.
- `grows_selection` (`:3800`) gains `HelixVerb::SelectObject`.
- Every surround verb goes through `apply_helix_edit` → `submit_edit`
  (`:1496`) → `controller.apply_local` (`:1530`) → `editor::push_undo`
  (`src/tui/editor.rs:4292`). **No bespoke transaction path.** `submit_edit`
  captures `selections_before` (`:1514-1518`) and pushes the undo entry
  (`:1566`), so an undo after a surround restores the selection set as well
  as the text.
- `m a` / `m i` are **selection** verbs: they set the selection and do not
  edit text, so they go through `apply_helix` (not `apply_helix_edit`) and
  push no undo entry.

### 6.7 The mismatch policy

`EditorPrefix::swallows_unmatched` (`src/tui/editor.rs:276`) is `true` only
for `Goto`. For `m`:

- An unmatched second key after `m` (e.g. `m z`) should **retry** — the key
  falls through to the composer — matching `space`/`[`/`]`. Helix swallows it,
  but the *editor-wide* rule is that a prefix which does not recognise the
  next key returns it to the composer rather than eating it. Swallowing
  would make `m` + a letter silently type nothing, which is the M002 failure
  mode. **Record the divergence** in the same place the `m m` divergence is
  recorded.
- After `m s`, an argument key that is **not** a known delimiter is still
  accepted: Helix's surround accepts any character as a delimiter. It is
  recorded as a one-character pair (open == close), which is what quotes
  need.

### 6.8 Guard changes

- `scripts/check_tui_editor_prefix_state.py` must learn the new variant. It
  asserts the *presence* of specific names, not exhaustiveness, so a new
  variant does not fail it — but the plan should add the new names to the
  guard's required-declarations list so the grammar cannot be deleted
  silently, and the guard's output must stay byte-comparable.
- `scripts/check_tui_editor_key_authority.py` needs **no** change: the `m`
  arm copies an existing parseable shape. The closure record states the new
  Normal-mode chord count (66 → 67 for the `m` arming arm; the `m`-prefix
  resolution itself is a leaf-table row and adds no chord to the guard's
  tables).

## 7. Ordered work packages

Each package is a coherent pass. The order matters: the grammar is useless
without the resolvers, and the resolvers are untestable without the grammar.

- **W1 — The byte foundation. LANDED.** The pair table and the depth-counting
  scanner, delivered as the dependency-free module `src/tui/editor_match.rs`
  (§6.4), with the bounded-window discipline and unit tests for unbalanced,
  nested, multi-byte and empty input. No UI. The module's own test suite is
  **33 tests**; it compiles and runs standalone with
  `rustc --edition 2021 --test src/tui/editor_match.rs -o /tmp/editor_match_tests && /tmp/editor_match_tests`,
  which is how it is verified — the module has no crate dependencies, so it
  needs neither the cargo build lock nor the crate's snapshot type.
- **W2 — The grammar.** `EditorPrefix::Match` and
  `EditorPrefix::MatchArg(MatchArg)`; `MATCH_LEAVES`; the `display()` arms;
  `is_capture`; the `m` arming arm in `handle_normal_key`; the resolver
  transitions; the mismatch policy. Tests at the `resolve_pending_prefix`
  level: `m` + each leaf resolves; `m` + an unknown glyph retries;
  `m s` + a character captures; `m r` + two characters captures in order.
- **W3 — Pair matching.** `EditorCommand::MatchBracket`; the geometry; the
  refusals with their distinct reasons; the `m m` leaf. Dispatch-path test.
- **W4 — Text objects.** The five object resolvers; the `SelectObject` verb;
  `PrefixAction`'s new arm; `set_members` with the `last_char_start` head
  rule; `grows_selection`. Dispatch-path tests for `m a` and `m i` against
  `w`, `W`, `p`, a pair character and `m`.
- **W5 — Surround.** The three surround verbs; the nesting rule; the
  selection growth/collapse; `mutates_buffer`; the undo funnel. Dispatch-path
  tests, including the nesting case `(a(b))` → `[a(b)]`.
- **W6 — Guards, docs and the sweep.** The prefix-state guard's required
  names; the insert-mode sweep count stays 93; the doc surfaces in §12; the
  `CHANGELOG.md` entry; the skill.

  Two guard details that are easy to get wrong. The prefix-state guard
  checks its needles as **plain substrings against the whole `src/` text**
  (`scripts/check_tui_editor_prefix_state.py:271-273`), so a bare `"Match"`
  needle would be satisfied by `matches!`, `Matching` or `MatchMode`
  appearing anywhere. Use **`"MatchArg"`**, which is distinctive, and
  rely on the existing `"pub enum EditorPrefix"` needle for the enum
  itself. And `every_pending_prefix_display_fits_the_indicator_bound`
  (`src/tui/editor.rs:6632`) lists **nine** prefixes and **omits
  `ReplaceChar`** — a pre-existing gap. Fix it here, and add the new
  variants to that list, or the new prefixes are never rendered.

## 8. Failure, cancellation, restart, contention semantics

- **No daemon, no protocol, no transport.** This milestone adds no I/O. There
  is nothing to cancel and no restart path beyond the existing document
  attachment reset.
- **Buffer switch / reload / resync** clear the presentation
  (`clear_history` at `src/tui/editor.rs:4400` disarms the prefix), so an
  armed `m` cannot survive into a different document. The existing
  `reset_for_open` path covers it.
- **A resolver that finds nothing must refuse with a reason**, never mutate
  the buffer and never panic. A malformed buffer is a normal state.
- **A resolver must not allocate unboundedly.** The bounded window is the
  contract; a resolver that allocates proportional to the document on a
  keystroke is a defect even if it terminates.
- **Read-only attachment.** `apply_helix`'s gate refuses before any
  transaction; the notice must name the reason.

## 9. Compatibility and migration

- **No protocol change.** `crates/codegg-protocol` is untouched.
- **No storage change.** No migration, no schema version bump.
- **No daemon change.** The daemon owns language-server interaction; none of
  it is involved.
- **The editor route's public surface grows** by two `EditorPrefix` variants
  and one `PrefixAction` arm. These are internal to the TUI crate; the
  `codegg-client` crate and the WS protocol are unaffected.
- **One doc-visible behaviour changes**: `m` stops being an unbound letter
  and becomes a prefix. `m` was previously unclaimed in the editor route, so
  no existing binding is displaced.
- **The insert-mode sweep count stays 93.** `m` is already a swept letter.

## 10. Required tests

Every bound chord is driven through the **real dispatch path** — `App::on_key`
→ `handle_editor_key` → `editor_buffer_key` → `handle_normal_key` — not
through a helper. The roadmap's §9 names the reason: *"Calling `editor_redo`
directly, as the existing tests do, is what let the `Ctrl+r` shadow survive."*
The M013 precedent added two such App-route tests for the same reason.

- **Prefix-level** (`src/tui/commands/editor.rs`):
  - `m` arms `Match`; `m` + each leaf glyph resolves to its action.
  - `m` + an unlisted glyph **retries** (the key reaches the composer).
  - `m s` + `(` captures a `SurroundAdd`; `m r` + `(` then `[` captures
    `SurroundReplaceFrom` then `SurroundReplaceTo`, in that order.
  - `m a` + `w` captures `AroundObject`; `m i` + `w` captures `InnerObject`.
  - A capture never resolves through the leaf lookup — the regression the
    `is_capture` invariant exists to prevent.
  - `display()` for every new variant stays within the indicator bound.
- **Resolver-level** (byte work, in `src/tui/editor.rs`):
  - word / WORD / paragraph at a cursor, at a line start, at a line end;
  - pair resolution for nested, unbalanced and multi-byte input;
  - the closest-pair resolver picks the **innermost** enclosing pair;
  - a bounded window: a resolver over a document larger than the budget
    refuses rather than scanning it all.
- **Dispatch-level** (App route, `src/tui/commands/editor.rs`):
  - `m m` on `(` jumps to `)`, and on `)` jumps to `(`;
  - `m m` on a non-delimiter refuses with a reason;
  - `m m` on an unbalanced delimiter refuses with the *unbalanced* reason;
  - `m s (` on a held selection surrounds it and grows the selection;
  - `m r ([` on `(a(b))` yields `[a(b)]` — **nesting preserved**;
  - `m d (` on `(abc)` yields `abc`;
  - `m a w` selects the word plus its trailing delimiter boundary; `m i w`
    selects the word without them;
  - `m i p` selects the paragraph's interior;
  - `m i (` selects a pair's interior; `m i m` selects the innermost pair's
    interior;
  - a surround on a read-only attachment is refused with a notice and the
    buffer is unchanged;
  - an undo after a surround restores both the text and the selection set.
- **Guard non-vacuity**: a deliberately duplicated `m` arm (or a captured
  variant removed from `is_capture`) must make the relevant guard fail. Prove
  it by injection before citing the guard as evidence.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261018-12255c45

# W1's module is dependency-free, so it is verified without cargo and
# without contending for the shared build lock.
rustc --edition 2021 --test src/tui/editor_match.rs -o /tmp/editor_match_tests \
  && /tmp/editor_match_tests

cargo fmt && cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --locked --bin codegg-sandbox-helper
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib --no-fail-fast

python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
bash scripts/verify.sh quick
```

`verify.sh full` is the wider sweep; the pre-existing 69 failing `tests/`
integration binaries are **not** this milestone's and are not fixed here (see
`plans/registry.md`, "Residual registered, not discharged by M013"). A
closure that claims a green full sweep must have one.

## 12. Documentation updates

| File | Section | What changes |
|---|---|---|
| `docs/tui.md` | `### Multi-key gestures` (`:845`) | describe the `m` grammar, its two levels, and its mismatch policy |
| `docs/tui.md` | `### Change verbs` (`:740`) | the three surround verbs |
| `docs/tui.md` | `### Selecting more than one region` (`:792`) | `m a` / `m i` |
| `docs/tui.md` | `### Goto mode` (`:861`) | the neighbouring prefix-mode table — the `m` table joins it |
| `architecture/tui.md` | `#### The pending-prefix grammar: one prefix, one terminal key` (`:3342`) | the two-level `m` inside a one-field machine — the `MatchArg` payload rule |
| `architecture/tui.md` | `#### The change verbs, and why p reads the system clipboard` (`:3725`) | surround verbs |
| `architecture/tui.md` | `#### The selection-manipulation verbs (M008)` (`:1312`) | `SelectObject` |
| `architecture/tui.md` | the `m m` pair-table divergence | recorded next to the other recorded divergences |
| `CHANGELOG.md` | `## [Unreleased]` → `### Added` | the new grammar, and the `m m` divergence |
| `.opencode/skills/tui/SKILL.md` | the editor-grammar bullet | the `m` grammar and the divergence, with `architecture/tui.md` as authority |

`AGENTS.md` requires the skill and the `architecture/` doc to move together.

## 13. Acceptance criteria (externally observable)

1. `m` arms a visible prefix indicator; `m` + each bound leaf produces its
   effect; `m` + an unknown key types the key normally.
2. `m m` on a bracket jumps to its mate; on a non-bracket and on an
   unbalanced bracket it says which of the two happened.
3. `m s (`, `m r ([` and `m d (` do what Helix does, including
   `(a(b))` → `[a(b)]`.
4. `m a w`, `m i w`, `m a W`, `m i W`, `m a p`, `m i p`, `m i (`, `m i m`
   each select the documented range.
5. A surround on a read-only attachment changes nothing and says why.
6. Undo after a surround restores the text **and** the selection set.
7. No object resolution scans more than the bounded window; a document
   larger than the budget is refused, not scanned.
8. Multi-byte content around a bracket or a word is neither corrupted nor
   mis-scanned.
9. The insert-mode sweep still asserts `inspected == 93`.
10. `scripts/verify.sh quick` passes, and the key-authority guard reports the
    new chord count.

## 14. Stop conditions

Stop and report rather than continue if any of these holds.

- **The pair-table `m m` cannot be made correct for unbalanced input**
  without a whole-buffer scan. The bounded window is not negotiable.
- **`PrefixAction` losing `Copy` causes a wider ripple than §6.2 predicts**,
  and the alternative (b) is materially cleaner. Re-derive the choice and
  record it; do not push a large refactor through a keybinding milestone.
- **A text object needs a syntax tree to be correct** — that is M014, not
  this milestone. Adding a parser dependency here would silently convert a
  capability milestone into a dependency decision, which is ADR-0034's job.
- **Any edit path bypasses `submit_edit`.** A bespoke transaction path would
  split the undo funnel and break the read-only gate.
- **The `m` prefix cannot be expressed with one presentation field.** The
  alternative is a second field, which the prefix-state guard rejects and
  which M002's design note forbids. That is an architecture decision, not an
  implementation detail.
- **A resolver's cost turns out to be super-linear in the document** for a
  common case.

## 15. Closure evidence required

- A closure record at
  `plans/closure/tui-editor-helix-parity/009-status.md` with: the baseline
  SHA, the work packages landed, the **new** key-authority chord counts, the
  guard outputs, the test names and counts for every suite in §11, and the
  explicit list of the milestones' exit conditions with the evidence for
  each.
- A **negative** statement for the `verify.sh full` sweep: if it was not
  green, say so and say why, and confirm the failing set is identical to the
  registered pre-existing residual.
- The `m m` divergence and the unmatched-key divergence, each with the doc
  reference that records it.
- The guard-injection proof from §10, with the command and the observed
  failure.
- A note on whether the roadmap's Amendment **A3** (the M009 scope split) and
  the M014 registration were applied in the same commit.

## 16. Handoff notes

- **The M014 slice is not this milestone's.** The syntax-node objects
  (`f`, `t`, `a`, `c`, `T`, `x`) and a syntax-tree `m m` are gated on
  ADR-0034's acceptance. ADR-0034 **recommends Option B** — extend the
  daemon-owned, read-only LSP surface with structural reads — but "no option
  is selected by this record", so **M014's implementation plan must be
  written after the ADR is accepted**, because Option A (a frontend
  tree-sitter dependency) and Option B (daemon-owned structural reads)
  produce *different contracts* and different plans. Writing M014 now would
  be writing a plan against a decision nobody has made.
- **The object vocabulary is deliberately small.** Five byte-derived objects
  (`w`, `W`, `p`, a pair character, `m`) is the whole of Helix's
  parser-free set. `ObjectKind` must not grow a variant that nothing
  constructs: a variant with no producer is a claim, not a hook.
- **The pair table is a product decision, not a technical one.** Which
  delimiters count as pairs is a judgement the reader will notice. The set in
  §6.4 matches Helix's and is the floor; a smaller set is defensible, a
  larger one is scope creep.
- **The prefix machine's constraint is the interesting part of this
  milestone.** One field, one terminal key, and a two-level Helix grammar —
  §6.1 resolves it with a payload in the enum, which keeps the
  presentation's field count at one and the prefix-state guard green. A
  reviewer should check that claim rather than trust it.
