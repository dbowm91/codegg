# TUI editor Helix parity Milestone 006 — Unimpaired navigation, non-LSP/non-TS subset

Status: implemented — **closed 2026-10-19**, see
`plans/closure/tui-editor-helix-parity/006-status.md`. All four chords are bound,
`PrefixAction` gained the `Edit` arm the plan called for, and the `[`/`]`
prefixes finally have leaf rows.
Repository baseline: `84440fe7f` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 M006
Long-term requirements: `plans/000-long-term-specification.md` (TUI editor);
`plans/002-long-term-roadmap.md` (editor capability sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
Primary class: capability

## 1. Objective

Land the half of Helix's unimpaired `[`/`]` layer that is **pure frontend
document work**, and widen `Alt-.` to repeat it:

- `]p` / `[p` — next / previous paragraph.
- `]Space` / `[Space` — add a newline below / above.
- `Alt-.` widened to repeat the unimpaired motions, closing the composition
  note the roadmap already records against M003's `repeat_last_motion`.

This milestone delivers the `[`/`]` **grammar** — the two prefixes that M002
armed with an empty leaf table become real grammars — and the first four
bindings that need nothing beyond the editor's own document replica.

> ## 1.1 Amendment A1 — three corrections against evidence (2026-10-18)
>
> This plan's first draft took the roadmap's M006 deliverable boundary at face
> value. Three independent checks — the live Helix keymap, a call-graph trace of
> the diagnostics read path, and a search for any reachable change/hunk data —
> showed the boundary cannot be delivered as written. Each correction is
> recorded here rather than absorbed, per `plans/003-planning-process.md` §2.4.
>
> ### A1.1 `]D`/`[D` and `]G`/`[G` are inverted in the roadmap
>
> The roadmap reads "`]D`/`[D` first/last" and "`]G`/`[G` first/last change".
> The live keymap reads:
>
> | Key | Helix label | Command |
> |---|---|---|
> | `]D` | Go to **last** diagnostic in document | `goto_last_diag` |
> | `[D` | Go to **first** diagnostic in document | `goto_first_diag` |
> | `]G` | Go to **last** change | `goto_last_change` |
> | `[G` | Go to **first** change | `goto_first_change` |
>
> `]` is *forward* throughout this layer (`]d` = next, `]p` = next, `]g` = next),
> so `]D` = last and `[D` = first. The roadmap's "first/last" pair is backwards
> for the `]` glyph. Helix is the normative source for these semantics; the
> successor milestone must implement the keymap, not the roadmap's prose.
>
> ### A1.2 The diagnostic quartet is **M013's**, not this milestone's
>
> Two independent findings, either of which alone forces the deferral:
>
> 1. **It overlaps M013's own deliverable boundary**, which already names
>    "`]`/`[` diagnostic navigation". The roadmap claims these bindings twice.
> 2. **The read path is broken today**, which M013 exists to repair. Traced at
>    this baseline: `publish_changed_diagnostics` and
>    `DIAGNOSTICS_POLL_INTERVAL` (`src/core/daemon_lsp.rs:555`, `:570`) have
>    **zero callers**, so `daemon.lsp_diagnostics` never receives a production
>    write and every diagnostics read is permanently `NotReady`; and
>    `find_existing_client_for_root_hint(Some(&root), None)`
>    (`src/core/daemon_lsp.rs:213-215`, `:334-336`) returns `Err` when more than
>    one client shares the root, mapped flatly to `NotReady`.
>
> This is the same reasoning the roadmap already applied to M010: *"Binding
> Helix LSP leaves before M013 lands would ship keys that open always-`NotReady`
> panels."* A `]d` that jumps nowhere is harder to diagnose than an absent key.
> `]d`/`[d`/`]D`/`[D` therefore move to M013, which owns both the pipeline repair
> and the binding. **This milestone must not bind them.**
>
> ### A1.3 The change quartet has no reachable data source
>
> The roadmap says `]g`/`[g`/`]G`/`[G` work by "reusing the existing diff
> surface". **There is no such surface the editor can reach.** Verified:
>
> - `crates/egggit/src/diff.rs` exposes `changed_files`, `diff_summary`,
>   `file_diff`, `diff_text`, `validate_patch`, but `ChangedFile { path, kind }`
>   and `FileDiff { path, patch }` carry **no line ranges**.
> - The only parsed hunk type in the TUI is `DiffHunk`
>   (`src/tui/components/diff.rs:27`), produced by the **private**
>   `DiffViewer::compute_diff` (`:66`) inside a widget. Nothing else can read it.
> - `FileDiffStatsResult` (`src/tui/file_diff.rs:27`) is a sidebar count only.
> - `handle_normal_key` receives `&mut EditorState`. `EditorState` has **no git
>   handle** and no base/disk copy of the document, so no change navigation can
>   resolve even if it wanted to.
>
> Delivering `]g` would mean a new App-level async request, a daemon response
> type, parsed hunk storage in presentation state, and a new IPC surface — a
> second ownership boundary inside a milestone the roadmap classes as a
> frontend capability. That violates `plans/003-planning-process.md` §5
> (milestone sizing). **`]g`/`[g`/`]G`/`[G` are deferred to a new milestone**,
> and the roadmap is amended to record the reason. The options for that
> milestone are enumerated in §5.

### 1.2 What "paragraph" means here

Helix does not define the term on the keymap page. The vim-unimpaired
convention it is styled after treats a paragraph as a blank-line-delimited
block of text. This milestone adopts that definition, and §6 specifies the
scanner so the choice is explicit rather than implied by a name:

- A **blank line** is one whose bytes are entirely ASCII whitespace.
- `]p` lands on the first non-whitespace byte of the **next** paragraph.
- `[p` lands on the first non-whitespace byte of the **previous** paragraph.
- Consecutive blank lines are one separator, not N separators.
- From inside a paragraph, `]p` moves to the *next* paragraph, never standing
  still; from a blank line, `]p` also moves forward.
- At the last paragraph, `]p` is a **no-op that is reported** (see §8); the same
  for `[p` at the first.

## 2. Why this milestone is ready

- **M002 — closed** (`plans/closure/tui-editor-helix-parity/002-status.md`).
  `OpenBracket` / `CloseBracket` exist as `EditorPrefix` variants with an
  arm/disarm/resolve contract and **no leaves**. This milestone only adds rows
  to a table that is already the dispatch path's shape.
- **M005 — closed**. The per-prefix leaf table (`EditorPrefix::leaves() ->
  &'static [PrefixLeaf]`, `PrefixAction`) landed with M005 and is the mechanism
  this milestone uses. No shape change is needed.
- **M003 — closed**. `Alt-.` and the last-motion record it walks exist; this
  milestone widens the repeatable set rather than introducing it.
- No ADR gates this scope. Every binding here is a pure cursor move or a single
  local edit over the frontend replica.

## 3. Current implementation evidence

### The prefixes are armed and already inert

`EditorPrefix` has ten variants (`src/tui/editor.rs:92-119`): `Idle`, `Goto`,
`Space`, `OpenBracket`, `CloseBracket`, `FindNext`, `FindPrevious`,
`FindTillNext`, `FindTillPrevious`, `ReplaceChar`.

- `[` → `OpenBracket` at `src/tui/commands/editor.rs:3137-3142`; `]` →
  `CloseBracket` at `:3143-3148`. Both are `if m == none` only — correct, since
  neither glyph is shifted.
- `is_capture` (`src/tui/editor.rs:161-163`) is `is_find() || ReplaceChar`, so
  `[`/`]` are **not** captures and correctly fall through to the leaf lookup.
- `leaves()` returns `&[]` for both (`src/tui/editor.rs:241-248`), so today any
  second key misses the table, reaches `swallows_unmatched`
  (`src/tui/editor.rs:272-274` — true only for `Goto`), and returns `Retry`.
  The behaviour is pinned by
  `the_unimpaired_prefixes_disarm_without_swallowing_the_next_key`
  (`src/tui/commands/editor.rs:5208-5250`).

Adding the first rows therefore converts `Retry` into `Resolve`, and **that test
must keep passing for an unbound second key**. It is the regression signal that
the mismatch policy survived the leaf table becoming non-empty.

### The leaf table (verbatim, M005's shape)

```rust
pub enum PrefixAction {
    Motion(EditorCommand),
    YankSelection,
}

pub struct PrefixLeaf {
    pub glyph: char,
    pub action: PrefixAction,
}
```

`GOTO_LEAVES` (`src/tui/editor.rs:329-366`) has seven rows; `SPACE_LEAVES`
(`:373-376`) has one. There is no unimpaired table.

**`PrefixAction` has two variants and neither can express this milestone.** A
paragraph motion *is* an `EditorCommand`, so `Motion` covers `p`/`[p`. But
`add_newline_below`/`above` is an **edit**, not a motion, and `YankSelection` is
neither. §6 adds the variant rather than bending an existing one.

### No paragraph concept exists

`rg -c -i 'paragraph' src/tui/editor.rs` → no match. Every other hit in the
repo is `ratatui::widgets::Paragraph`. There is no blank-line scanner.

The available line helpers in `src/tui/editor.rs`, by name: `cursor_line`
(`:1237`), `line_start` (`:1246`), `line_byte_len` (`:1254`),
`line_is_truncated` (`:1263`), `line_content_end` (`:1293`),
`buffer_content_end` (`:1315`), `last_line_of` (`:1325`),
`first_non_whitespace` (`:1348`), `move_vertical` (`:1572`),
`next_line_content_start` (`:1808`, private), `half_page_target` (`:1828`),
`offset_on_line` (`:1839`), `clamp_offset` (`:1203`), `next_char_boundary`
(`:1213`), `prev_char_boundary` (`:1225`).

`line_content_end` is the model for a bounded line helper: it inspects at most
the last two bytes so a multi-megabyte line costs a constant, and it has a test
that would fail on an unbounded scan. The paragraph scanner follows it.

### The diagnostics path exists and is broken

F2 → `refresh_editor_diagnostics` (`src/tui/commands/editor.rs:537`) →
`EditorLspState::begin_diagnostics` (`src/tui/editor_lsp.rs:91`) →
`CoreRequest::LspDiagnosticsGet` (`:568-573`) →
`TuiCommand::EditorDiagnosticsFinished` (`:589`) →
`apply_editor_diagnostics` (`:605`) → `EditorLspState::apply_diagnostics`
(`src/tui/editor_lsp.rs:110`).

`EditorLspState.diagnostics` is `Vec<LspDiagnosticDto>` capped at
`MAX_EDITOR_DIAGNOSTICS = 128` and filtered to the active scope's path
(`src/tui/editor_lsp.rs:55`, `:11`, `:133-136`). `LspDiagnosticDto`
(`crates/codegg-protocol/src/lsp.rs:328-339`) carries an `LspRangeDto`
(`:195-206`) whose coordinates are `u32` **line / UTF-16 column**, not byte
offsets. A jump primitive already exists — `jump_to_selected_diagnostic`
(`src/tui/commands/editor.rs:2278-2311`), reachable from the panel's `Enter`
(`:2256-2258`) — and converts UTF-16 columns via `utf16_column_to_byte`.

**Two facts M013's `]d` work will need that this milestone should not have to
rediscover:** `apply_diagnostics` does **not** sort (only semantic tokens are
sorted, `src/tui/editor_lsp.rs:331`), so "next diagnostic" has no defined order
until one is chosen; and `NotReady` *preserves* the previous list while setting
`diagnostics_stale` (`:153-159`), so a stale-but-non-empty list is
indistinguishable from a fresh one unless a stale marker is checked. Both are
recorded in §5 as M013's to decide.

### Insert-mode integrity for `[`/`]` is unproven

`handle_insert_key` (`src/tui/commands/editor.rs:2587`) has no `[`/`]` arm and
`editor_buffer_key` routes Insert before Normal, so both are literal text today.
**No test asserts it.** `printable_characters_reach_the_document_in_insert_mode`
(`:4911`) sweeps `a`–`z` in three encodings plus
`'/': ':' ' ' '\n' '\t' '.' ',' '$' '0' '~' '_' '*' '?'` (`:4945-4947`) — `[`
and `]` are **absent**. `rg -l "Char\('\['\)|Char\('\]'\)" tests/` → zero hits.
This milestone closes that gap, because `[`/`]` are precisely the glyphs whose
Normal-mode arm is about to become non-empty.

## 4. Invariants that must not regress

1. **Unclaimed-key fallthrough.** In Normal mode, a key the buffer does not own
   returns `BufferKey::Unhandled`. The paragraph leaf uses `p`; the bare `p`
   arm (paste) is reached only when **no** prefix is armed, because
   `resolve_pending_prefix` runs first (`src/tui/commands/editor.rs:2889-2893`).
2. **Prefix mismatch semantics.** An unbound second key after `[` or `]`
   disarms and resolves **normally**, per M002. The existing test at `:5208`
   must pass unmodified — it is the acceptance signal for A1's scope change.
3. **Insert-mode text integrity.** `[` and `]` must remain literal text in
   Insert. This milestone adds the missing test rather than assuming it.
4. **Char boundary safety.** Paragraph landing positions must be `char`
   boundaries. `first_non_whitespace` must not stop mid-codepoint; the M003
   `read_bytes` defect is the standing hazard.
5. **Bounded line scans.** The paragraph scanner must not read an entire line.
   Follow `line_content_end`: a bounded prefix read plus a test that fails on an
   unbounded scan. `MAX_RENDER_LINE_BYTES` is the existing bound; a line past it
   is already drawn truncated, so the answer is disclosed rather than silently
   wrong.
6. **One transaction per edit.** `]Space`/`[Space` must route through
   `editor_editing_command` → `submit_edit` → `apply_local`, like every other
   edit verb. Never a second write path.
7. **Text authority.** No filesystem read, no retained text buffer. Owned by
   `scripts/check_tui_editor_text_authority.py`, which this milestone must
   leave passing.
8. **Shared prefix machinery.** The leaves go in the table. A new
   `TuiDocumentPresentation` field whose name contains `prefix` would fail
   `scripts/check_tui_editor_prefix_state.py` (`:226-241`) — that is the guard
   working, not an obstacle.

## 5. Scope

### In scope

- `OPEN_BRACKET_LEAVES` / `CLOSE_BRACKET_LEAVES` tables.
- `]p` / `[p` paragraph motions.
- `]Space` / `[Space` add-newline-below / above.
- `Alt-.` widened to repeat the unimpaired motions.
- The bound-outcome report for `]p`/`[p` at the document ends.
- Insert-mode integrity coverage for `[` and `]`.
- Guard model prose, `docs/tui.md`, `architecture/tui.md`.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| `]d`/`[d`/`]D`/`[D` | M013's stated boundary, and the read path is broken until M013 repairs it (A1.2) | **M013** |
| `]g`/`[g`/`]G`/`[G` | No reachable data source; needs git hunk IPC (A1.3) | **New milestone**, needs fresh planning |
| `]f`/`[f`, `]t`/`[t`, `]a`/`[a`, `]c`/`[c`, `]T`/`[T` | `TS`-marked; no tree-sitter in the workspace | ADR-0034 (proposed) |
| `space p`/`space P` | Belongs to the milestone that owns `SPACE_LEAVES` | M010 |

### Options for the change-navigation milestone

Recorded so the successor can be planned with evidence rather than from scratch.
Every option needs changed-line ranges **in the editor's own coordinate space**,
and none of them exists today:

1. **Daemon-side, git-derived.** Extend the existing document/diff read path to
   return changed line ranges for the open path. Cheapest at keystroke time;
   needs a protocol extension and a daemon-side git read.
2. **Frontend-side, on open.** Ask once when the document is opened and cache the
   hunk headers. No per-keystroke cost; goes stale as the buffer is edited.
3. **Frontend-side, against the file on disk.** Read the working file and diff
   it against the base commit. Direct, but puts a filesystem read and a process
   question on the editor surface, which §3's authorization invariants forbid.

Option 1 is the only one that keeps the editor free of filesystem authority.
That choice is the milestone's first acceptance criterion, not this plan's.

## 6. Required production changes

### `PrefixAction` gains an edit variant

`PrefixAction` becomes three-variant:

```rust
pub enum PrefixAction {
    Motion(EditorCommand),
    YankSelection,
    Edit(EditorCommand),   // routes through editor_editing_command
}
```

`Edit` is required because `add_newline_below` / `add_newline_above` mutate text
and must go through the same gate, snapshot, `EditorEdit`, and `submit_edit`
funnel as `d` and `D`. The dispatch arm reads the variant once and routes;
there is no second edit path. **Reading the current enum and folding these into
`Motion` because "they're commands too" is the specific mistake to avoid** —
it would route a mutation through the motion path, which does not check
`editable`.

### The two leaf tables

```rust
const OPEN_BRACKET_LEAVES: &[PrefixLeaf] = &[
    PrefixLeaf { glyph: 'p', action: PrefixAction::Motion(EditorCommand::NextParagraph) },
    PrefixLeaf { glyph: ' ', action: PrefixAction::Edit(EditorCommand::AddNewlineBelow) },
];
const CLOSE_BRACKET_LEAVES: &[PrefixLeaf] = &[
    PrefixLeaf { glyph: 'p', action: PrefixAction::Motion(EditorCommand::PrevParagraph) },
    PrefixLeaf { glyph: ' ', action: PrefixAction::Edit(EditorCommand::AddNewlineAbove) },
];
```

**Ordering matters for `]Space`.** The second key is a `Space` character, which
is also the glyph that arms the `Space` prefix. Because the pending prefix is
resolved before any bare-key arm, `]` then `Space` reaches this row and never
arms `SPACE_LEAVES`. `resolve_pending_prefix` must look up the leaf by glyph
**before** considering whether that glyph is itself a prefix starter — the same
precedence `:` and `/` already have over a pending target since M003. A test
must pin this, because the alternative ordering makes `]Space` arm space mode.

### Paragraph scanning (`src/tui/editor.rs`)

- `line_is_blank(snapshot, line) -> bool` — bounded: read at most the same
  prefix `line_content_end` reads, treat an all-whitespace prefix as blank only
  after the bound, and disclose truncation the way `first_non_whitespace` does.
- `next_paragraph_start(snapshot, from) -> Option<usize>` — scan forward line by
  line for the first line that is non-blank **and** is preceded by at least one
  blank line; return `first_non_whitespace` of that line. `None` when the
  document has no later paragraph.
- `prev_paragraph_start(snapshot, from) -> Option<usize>` — scan backward for
  the first line that is non-blank and preceded by a blank line, **strictly
  before** the current paragraph's start.

Both must skip runs of blank lines as one separator and must return `char`
boundaries only. `Option`, not a clamped offset, is what makes the end-of-file
case a reportable outcome rather than a silent no-op.

### `EditorCommand` additions

`NextParagraph`, `PrevParagraph`, `AddNewlineBelow`, `AddNewlineAbove`. The two
edit commands resolve through `resolve_edit`'s existing siblings; the two
motion commands resolve through the raw motion path, as M005's goto leaves do,
so they inherit `gg`'s collapse-selection behaviour rather than inventing a
second one.

### `Alt-.` widening

M003's last-motion record currently covers the find family. Helix's label reads
*"Repeat last motion (`f`, `t`, `m`, `[` or `]`)"*. `m` is match mode (M009) and
the unimpaired leaves exist as of this milestone, so the repeatable set becomes
find ∪ unimpaired. `Alt-.` after `]p` repeats the paragraph motion; after
`]Space` it does **not** — `Alt-.` repeats motions, and an edit is not one.
Record that asymmetry explicitly rather than leaving it to be discovered.

## 7. Ordered work packages

**W1 — Leaf tables and the `PrefixAction::Edit` variant.**
Add both tables and the new variant. Prove the mismatch policy survived by
running `the_unimpaired_prefixes_disarm_without_swallowing_the_next_key`
**unmodified**.
Acceptance: `cargo test -p codegg --lib tui::commands::editor` green with that
test unchanged; `check_tui_editor_prefix_state.py` exit 0.

**W2 — Paragraph scanner.** `line_is_blank`, `next_paragraph_start`,
`prev_paragraph_start`, the two motion commands, the `p`/`[p` rows.
Include the bounded-scan test mirroring `first_non_whitespace`'s.
Acceptance: `cargo test -p codegg --lib tui::editor` green; the scan test fails
if the bound is removed.

**W3 — Add-newline edits.** The two edit commands, the two `Space` rows, the
precedence test from §6, and a read-only refusal test asserting
`state.notice` is set.
Acceptance: focused tests green.

**W4 — `Alt-.` widening.** Extend the record and its dispatch, plus the
"an edit does not become the repeated motion" test.
Acceptance: focused tests green; M003's find-repeat tests unmodified.

**W5 — Insert-mode integrity for `[`/`]`.** Extend
`printable_characters_reach_the_document_in_insert_mode` with `[` and `]`, and
add the same pair to the M005 no-capture sweep for symmetry.
Acceptance: the sweep would fail if either glyph were claimed in Insert.

**W6 — Guards and docs.** Guard model prose for the two new tables and the
`Edit` variant; `docs/tui.md` keymap rows; `architecture/tui.md` capture/leaf
section and the two deferred slices.
Acceptance: all three editor guard scripts exit 0.

**W7 — Full verification.** §11.

### 7.1 Reportable outcomes

Every `]p`/`[p` at a document end, and every `]Space`/`[Space` on a read-only
buffer, sets `state.notice` (`src/tui/app/state/editor.rs:274` — a **field**,
set via `set_notice`, cleared via `clear_notice`). The M004 precedent applies:
"you copied nothing" and "`p` did not work" must not be indistinguishable. A
motion that cannot move must say so once and visibly.

## 8. Failure, cancellation, restart, contention semantics

- **At the last (first) paragraph**, `]p` (`[p`) is a no-op plus a notice. It
  must not wrap — Helix does not wrap, and wrapping would make the end of a
  document indistinguishable from the middle.
- **On a read-only buffer**, `]Space`/`[Space` are refused through the same
  `editable` gate as every other edit, producing the same
  `EditorNotice::from_error(&DocumentControllerError::ReadOnly)`. The notice is
  the answer, not silence.
- **On reload, buffer switch, or pane focus change**, the armed `[`/`]` prefix
  must disarm exactly as `g` and `space` do. A stale prefix must never apply to
  a newly opened buffer — the roadmap's §8 concurrency requirement.
- **No verb here is cancellable mid-motion.** There is no partial paragraph
  state to unwind.

## 9. Compatibility and migration

None. No protocol, storage, or configuration change. `PrefixAction` is internal
to the editor module and covered by the prefix-state guard, which needs its
model prose updated rather than its assertions relaxed.

## 10. Required tests

- `the_unimpaired_prefixes_disarm_without_swallowing_the_next_key` **unmodified**
  after W1 — the regression signal for the mismatch policy.
- `]p` / `[p`: single paragraph; two paragraphs; a run of three blank lines as
  one separator; blank first line; blank last line; a document that is entirely
  blank; a paragraph starting with a multi-byte character; `]p` from inside a
  paragraph moves forward rather than standing still.
- Paragraph end-of-document: `]p` at the last paragraph and `[p` at the first
  each leave the cursor where it was **and** set `state.notice`.
- Paragraph bounded scan: a multi-megabyte indented line, mirroring the existing
  `line_content_end_does_not_read_a_huge_line`.
- `]Space` / `[Space`: below, above, at the last line, on the last line without
  a trailing newline, and on a read-only buffer (notice set, buffer unchanged).
- **Precedence:** `]` then `Space` adds a newline and does **not** arm space
  mode; `Space` alone still arms it.
- `Alt-.` after `]p` repeats the paragraph motion; after `]Space` it repeats the
  *last motion*, not the edit. M003's find-repeat tests unmodified.
- Insert-mode: `[` and `]` reach the document as text, sweeping both the
  `NONE` and `SHIFT` encodings.
- `gg`, `space y`, `gs`, and the goto leaves are unchanged.

## 11. Required verification commands

```bash
cargo fmt
cargo test -p codegg --lib tui::editor
cargo test -p codegg --lib tui::commands::editor
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh quick
```

`verify.sh full` is required before closure. Never `--all-features`.

## 12. Documentation updates

- `docs/tui.md` — the four bindings in the keymap, the paragraph definition,
  and the fact that the diagnostic and change layers are absent rather than
  silently unbound.
- `architecture/tui.md` — the two leaf tables, `PrefixAction::Edit`, the
  leaf-before-prefix-arm precedence rule, and the three deferred slices with
  their owners.
- **Terminal-panel parity.** `src/tui/commands/interactive_terminal.rs` has no
  `helix_selection` import at all and calls only the panel's own wrapper methods,
  so it is **unaffected**. `src/tui/interactive_terminal.rs` speaks the same
  dialect for selection and navigation but has no unimpaired layer. Record the
  panel as **explicitly exempt** for this milestone rather than leaving the
  decision silent — the roadmap forbids silent divergence.

## 13. Acceptance criteria (externally observable)

1. `]p` and `[p` land on the first non-whitespace byte of the next/previous
   paragraph, treating a run of blank lines as one separator, on `char`
   boundaries.
2. At the last (first) paragraph the cursor does not move and a notice is shown.
3. `]Space` and `[Space` insert a newline below/above through the normal
   `submit_edit` funnel, and are refused with a notice on a read-only buffer.
4. `]` then `Space` adds a newline and does not arm space mode; `Space` alone
   still arms it.
5. An unbound second key after `[` or `]` still disarms and resolves normally —
   proven by the M002 test passing unmodified.
6. `Alt-.` repeats the last unimpaired motion and does not repeat an edit.
7. `[` and `]` remain literal text in Insert, proven by a sweep that would fail
   if either were claimed.
8. No paragraph scan reads an entire line; the bound test fails without it.
9. `]d`/`[d`/`]D`/`[D` and `]g`/`[g`/`]G`/`[G` are **not bound** — asserted by a
   test that a bracketed diagnostic key disarms and resolves normally, so a later
   milestone cannot land them by accident.
10. All three editor guard scripts and `verify.sh quick` pass.

## 14. Stop conditions

Report rather than improvise if:

- Adding the leaf rows would require editing or weakening
  `scripts/check_tui_editor_prefix_state.py` rather than updating its model
  prose.
- The mismatch test at `src/tui/commands/editor.rs:5208` needs modification.
  That means the leaf lookup changed behaviour for unbound keys, which is the
  one thing this milestone must not do.
- The paragraph scanner cannot be written inside the `line_content_end` bound
  without disclosing truncation in a way that makes `]p` visibly wrong on a
  large line. Report — a silently wrong answer on a large file is worse than
  no binding.
- Leaf lookup would need to move **after** prefix arming to make `]Space` work.
  That inverts M003's established precedence for `:`/`/`; report rather than
  reorder a shared rule.
- Any binding here turns out to need a filesystem read, a git handle, or a new
  App-level request. Stop: that is a different milestone's ownership boundary.

## 15. Closure evidence required

Implementation commits; §11 output labelled local vs CI; explicit confirmation
that the M002 mismatch test and M003's `Alt-.` tests passed **unmodified**; the
terminal-panel exemption decision from §12; a guard demonstration that
`check_tui_editor_prefix_state.py` fails on an injected violation (e.g. renaming
`fn is_capture(`) and that `check_tui_editor_key_authority.py` fails on an
injected shadow; and the recorded reason each deferred slice was deferred, so
the successor milestone inherits the reasoning rather than rediscovering it.

## 16. Handoff notes

Two things in this plan are judgement calls a reviewer should check first.

The **scope reduction** (A1) is the significant one. The roadmap's M006
deliverable boundary claimed six families; this plan delivers two and names
where the other four went. That is a deliberate correction against evidence, in
the same shape as M004's `R` amendment and M005's `gt`/`gc`/`gb` deferral. If a
reviewer prefers the roadmap's original boundary, the honest outcome is that M006
splits into three milestones — this one, a diagnostics-navigation milestone
merged into M013, and a change-navigation milestone needing a data-source
decision — not that the four deferred bindings are implemented now.

The **paragraph definition** in §1.2 is likewise a judgement call, because
Helix does not define the term. It is specified explicitly so the tests can
assert behaviour rather than a name, and so a reader who disagrees can change the
definition in one place instead of discovering a different one inside the
scanner.
