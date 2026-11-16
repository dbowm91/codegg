# TUI Render Cost Milestone M005 — The Editor's Draw Path

Status: ready for handoff

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M005)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "Document editor (M006-A)", "The editor's Helix UX
  surface", "Bounded rendering and tree scheduling (C002)", and "The render
  path's caches (C006)"
- `architecture/document.md`

Applicable ADRs:

- **None required.** No new ADR. Accepted ADR-0011 (the editor holds no second
  text buffer) is governing and is **not reopened**: every change here reads the
  authoritative `DocumentSnapshot` the same way it does today, and none
  introduces a second buffer, a materialized `Vec<Line>`, or a cached document
  copy. `scripts/check_tui_editor_text_authority.py` must stay green.

Primary class: **polish** (cost)

## 1. Objective

A visible editor row is read and walked only as far as the column window needs,
the semantic-token and syntax-style lookups are bounded by the row rather than
by the token set, and the highlight cache's prefix check stops re-reading and
re-hashing the whole scanned prefix on every keystroke.

## 2. Why this milestone is ready

No hard dependency. C006 already re-keyed the highlight cache on per-line
content, which is the primitive this milestone extends; the cache's
`first_changed_line` scan and the draw path's per-row read are the costs that
re-keying left behind.

Its interface dependencies are stable and read-only: `DocumentSnapshot`'s
`read_bytes` / `line_range` / `line_start` API, the `TextSource` trait's
`byte_at`, and `EditorViewport`'s shape. None changes.

## 3. Current implementation evidence

Line numbers are at the baseline. Re-derive them.

### 3.1 A visible row is read whole and walked past the column window

`src/tui/components/editor.rs:1416` `line_spans`, at `:1454`:

```rust
let Ok(line) = self.snapshot.read_bytes(start, end) else {
    return vec![];
};
...
let limit = viewport.column.saturating_add(viewport.text_columns as usize);
for (byte, ch) in line.char_indices() {
    let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
    let next = display.saturating_add(width);
    if !started && next > viewport.column {
        visible_start = byte;
        started = true;
    }
    if started && display < limit {
        visible_end = byte + ch.len_utf8();
    }
    display = next;
}
```

Two costs, both per visible row per frame:

1. `read_bytes` allocates a `String` copy of the **whole line**, up to the
   per-line render budget.
2. The loop has **no early exit**. Once `display >= limit`, `visible_end` stops
   advancing but the walk continues to the end of the line, decoding every
   character. The row is drawn from `visible_start..visible_end`; the rest of the
   walk is thrown away.

The cost is therefore `O(sum of visible rows' line bytes)` per frame rather than
`O(visible columns)`.

### 3.2 The viewport resolver reads the same window a second time

`src/tui/editor.rs:5536` `resolve_viewport` computes the horizontal clamp with
`longest_visible_width(snapshot, first, visible_lines)` (`:5636`), which loops
over every visible line and calls `display_width` (`:5602`) — and `display_width`
calls `read_bytes`, allocating a `String` per line. Bounded by
`MAX_FRAME_SOURCE_BYTES` across the window, but still a full second read of the
visible window on every frame, immediately before `line_spans` reads the same
lines again.

### 3.3 The semantic-token scan is linear over all tokens for every row

`src/tui/components/editor.rs:1501`:

```rust
for token in self
    .lsp
    .semantic_tokens
    .iter()
    .filter(|token| token.line as usize == line_index)
```

Tokens are sorted by `(line, character)` — `src/tui/editor_lsp.rs` validates and
orders them — so this is `O(visible rows × all tokens)` per frame when
`O(log n + tokens on the row)` suffices. `MAX_EDITOR_SEMANTIC_TOKENS` is the
protocol bound (`codegg_protocol::lsp::MAX_LSP_SEMANTIC_TOKENS`, 1000).

The file already contains the right pattern: the search-match lookup at `:1553`
uses a partition/range approach on sorted data.

### 3.4 The style lookup is quadratic in a row's segments

`src/tui/components/editor.rs`, in the per-segment loop:

```rust
let style = syntax_ranges
    .iter()
    .find(|(token_range, _)| {
        token_range.start <= range.start && token_range.end >= range.end
    })
    .map_or(base, |(_, token_style)| base.patch(*token_style));
```

`syntax_ranges` and the boundary list are both ascending, but the search starts
from index 0 for every segment. For a dense line (a minified file under the
per-line budget, thousands of tokens) that is `segments × ranges` comparisons per
row per frame.

### 3.5 The highlight cache re-reads and re-hashes the whole prefix per keystroke

`src/tui/editor_highlight.rs:215` `first_changed_line`:

```rust
for index in 0..self.scanned_to {
    let Some(cached) = self.line_hashes.get(&index) else { return Some(index) };
    if self.line_hash(snapshot, index) != Some(*cached) {
        return Some(index);
    }
}
```

and `line_hash` (`:229`) does `line_range` + `read_bytes` — a `String` copy of
every line — then hashes it. `rebase` (`:203`) then replays the parser over the
same prefix (`replay_to`).

So editing line 4,900 of a 5,000-line file (`MAX_HIGHLIGHT_SCAN_LINES = 5_000`,
`:61`) re-reads, re-hashes and re-highlights ~4,900 lines **per keystroke**.

### 3.6 The cache key is re-allocated per visible row

`src/tui/editor_highlight.rs:164`:

```rust
let key = ScanKey { path: path.to_string(), code_theme: code_theme.to_string() };
if self.key.as_ref() != Some(&key) {
    self.reset();
    self.key = Some(key);
}
```

`spans_for_line` is called once per visible row per frame, so a 40-row viewport
allocates 80 `String`s per frame to re-derive a key that did not change.

### 3.7 The cursor's line is recomputed per row

`render_text` (`src/tui/components/editor.rs:671`) resolves `cursor_line` once at
`:675`, then passes only `line_index` down. `head_cells` recomputes it at `:1756`
for every selection, and `cursor_cell` recomputes it at `:1794` per row. Each is
a rope `line_of_byte` lookup that the frame already performed.

### 3.8 `TextSource::byte_at` allocates a `String` per scanned byte

`src/tui/text_geometry.rs:558`:

```rust
fn byte_at(&self, offset: usize) -> Option<u8> {
    if !self.is_byte_boundary(offset) { return None; }
    let end = next_char_boundary(self, offset).unwrap_or(self.len_bytes());
    self.read_bytes(offset, end).ok().and_then(|text| text.as_bytes().first().copied())
}
```

`read_bytes` returns `String`. `classify_byte`/`is_word_byte` callers
(`:242`–`:405`) call `byte_at` once per scanned byte, so an *n*-byte run costs *n*
small allocations. This is the **keystroke** path, not the frame path: the `str`
and `ComposerText` adapters override `byte_at` correctly, so only the snapshot
adapter is affected. It is in scope because it is the same "derive a byte by
allocating a string" shape and the fix is local.

## 4. Invariants that must not regress

- **ADR-0011: the editor holds no second text buffer.** No `DocumentBuffer`, no
  `Rope`, no materialized `Vec<String>`/`Vec<Line>` field, no cached document
  copy. `scripts/check_tui_editor_text_authority.py` must stay green.
- **The rendered output is byte-identical** for a given state, including the
  horizontal clip, the gutter, the current-line tint, the selection bands, the
  truncation suffix, and the semantic-token override.
- **The bounded-rendering invariant (C002) is unchanged**: a cursor at the far
  end of a multi-megabyte line still costs a bounded prefix, not the whole line.
  This milestone makes the bound tighter, not looser.
- **The highlight cache's content keying is unchanged** — a line whose bytes are
  unchanged keeps its spans, and a changed line is re-coloured from the context
  it would have been coloured from. C006's
  `tui_render_cost::a_local_edit_keeps_the_highlights_of_the_lines_above_it`
  must keep passing.
- **The keyed hash stays keyed.** `DefaultHasher` is used deliberately so a
  deliberate collision is not cheap (C006 §9). Do not replace it with an
  unkeyed hash.
- **One producer of a row's spans.** The cached spans remain the only source;
  this milestone changes how a row's window is computed, not where its styles
  come from.

## 5. Scope

### In scope

- `line_spans`' per-row read and its character walk.
- `resolve_viewport`'s `longest_visible_width` second read.
- The semantic-token scan.
- The per-segment `syntax_ranges` lookup.
- `first_changed_line`'s prefix re-read and re-hash.
- `ScanKey`'s per-row allocation.
- The repeated `cursor_line` computation in `head_cells` and `cursor_cell`.
- `TextSource::byte_at`'s snapshot adapter.
- Counters: `editor_row_reads`, `editor_row_bytes`, `editor_token_scan_steps`,
  `highlight_prefix_lines`.

### Explicitly out of scope

- The highlight engine (`syntect`) and its forward-only state. Its fields are
  private, so the replay cannot be checkpointed; this milestone reduces the
  **re-read** and the **prefix length**, not the replay.
- `src/tui/wrap.rs` — M006.
- The editor's key dispatch, the prefix grammar, the leaf tables, and the
  completion popup.
- The LSP request path, the semantic-token validation, and the token cap.
- The document buffer, the undo history, and `TextTransaction`.
- Any behaviour change to horizontal scrolling or the truncation suffix.
- The desktop editor.

## 6. Required production changes

### Core/domain

- `line_spans` must stop walking past the column window: break the character loop
  once `display >= limit` and the window's end is settled, keeping the
  `viewport.column == 0` and `!started` arms exactly as they are. It must also
  read only the prefix the window needs, not the whole line — the implementer
  must confirm that `visible_start..visible_end` can be computed from a bounded
  prefix without changing the clip for a multi-byte character that straddles the
  boundary.
- `resolve_viewport` must not re-read the visible window for the horizontal
  clamp on a frame where the window, the width and the document did not change.
  The implementer must cache `longest_visible_width` keyed on
  `(revision, first_line, visible_lines, width)` — the value cannot change unless
  one of those does. A cache that needs the document's bytes retained is a stop
  condition (ADR-0011).
- The semantic-token scan must use `partition_point` (or an equivalent) on the
  sorted token vector to find the row's range, then iterate only that range.
- The `syntax_ranges` lookup must become a single sweep: both `syntax_ranges` and
  the boundary list are ascending, so one index advances monotonically while the
  segments are walked. `find` must not remain inside the loop.
- `first_changed_line` must start from the edit rather than from line 0 where the
  presentation knows the edited range, and must fall back to the 0-based scan
  only when it does not. It must count bytes rather than materialize each line —
  a non-allocating byte-range comparison against the cached hashes, or a hash
  computed over a borrowed slice, either way without a `String` per line.
- `ScanKey`'s comparison must not allocate on a hit: compare the `&str` fields
  against `self.key` first and allocate only on a miss.
- `head_cells` and `cursor_cell` must take the already-resolved `cursor_line`
  as a parameter instead of recomputing it.
- `TextSource::byte_at`'s snapshot adapter must not allocate: add a
  non-allocating byte accessor on the snapshot (or fetch the line once per motion
  and index the slice).

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The editor is one widget on the render thread.

### Frontend or operator surface

- Byte-identical rendering for the fixture set in §10.

### Security and authorization

No authorization boundary. Two checks:

- The highlight cache is keyed on line **content** with a keyed hash. Reducing
  the prefix it re-reads must not reduce what it verifies: a changed line above
  the edit must still be detected. The invalidation test must cover an edit
  **above** the cached scan and an edit **below** it.
- No path, document content, or buffer byte is retained beyond the widget's
  existing lifetime. The `longest_visible_width` cache holds an integer, not
  content.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": the
  `longest_visible_width` cache with its key, and the note that the highlight
  cache's prefix is verified from the edit.
- `architecture/tui.md` §"Bounded rendering and tree scheduling (C002)": confirm
  the bound still holds and is now tighter; do not restate it.
- `src/tui/render_cost.rs`: the four counters.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter, beside
  `a_local_edit_keeps_the_highlights_of_the_lines_above_it`.
- `scripts/check_tui_editor_text_authority.py` must stay green with no
  allowlist change. If it needs one, stop — that is ADR-0011 being reopened.

## 7. Ordered work packages

### WP1 — Baseline counts

Intent: a before for every claim.

Required changes: add the four counters with no behaviour change; run the new
assertions against the unmodified tree and record the failures.

Acceptance evidence: baseline counts for a 40-row viewport over short lines, over
long lines, and over a minified line; a 5,000-line document with an edit at line
10, at line 2,500 and at line 4,900; a document with 1,000 semantic tokens.

### WP2 — Bound the per-row walk

Intent: the row's cost must follow the columns drawn, not the line's length.

Required changes: the early exit and the bounded read.

Acceptance evidence: `editor_row_bytes` is bounded by the drawn window plus one
character, not by the line's length, for a long-line fixture; the rendered buffer
is identical for a line whose clip lands mid-character, mid-escape, and on a
wide character.

### WP3 — Cache the horizontal clamp

Intent: remove the second full read of the visible window.

Required changes: the cache in `resolve_viewport`, keyed on
`(revision, first_line, visible_lines, width)`.

Acceptance evidence: `editor_row_reads` drops by the viewport's height per
unchanged frame; a document change, a scroll, a resize and a horizontal
re-resolution each rebuild. The rendered buffer is identical at several scroll
positions.

### WP4 — Bound the token and style lookups

Intent: `O(rows × tokens)` and `O(segments × ranges)` become `O(rows log tokens +
tokens on row)` and `O(segments + ranges)`.

Required changes: `partition_point` for the token range; a single sweep for the
style lookup.

Acceptance evidence: `editor_token_scan_steps` is bounded by tokens on the row
plus a logarithmic term, not by all tokens; the rendered buffer is identical for
a fixture with overlapping-looking token ranges, adjacent tokens, and a token
that does not cover a whole segment.

### WP5 — Verify the highlight prefix from the edit

Intent: editing near the end of a 5,000-line file must not re-read 4,900 lines.

Required changes: seed the check from the edit where known; count bytes rather
than materializing lines.

Acceptance evidence: `highlight_prefix_lines` is bounded by the distance from the
edit, not by `scanned_to`, for an edit near the end; an edit above the scan and an
edit below it are both still detected (the two directions are separate tests);
`a_local_edit_keeps_the_highlights_of_the_lines_above_it` still passes.

### WP6 — Stop the per-row key allocation and the repeated cursor line

Intent: two small, obvious wins.

Required changes: compare before allocating in `ScanKey`; thread `cursor_line`
into `head_cells` and `cursor_cell`.

Acceptance evidence: no `String` allocation per visible row for the key; the
current-line tint and the cursor cell are identical.

### WP7 — Non-allocating `byte_at`

Intent: an *n*-byte word motion must not make *n* allocations.

Required changes: the snapshot adapter.

Acceptance evidence: word motions and `w`/`b`/`e` produce the same landing bytes
as before, including across a multi-byte character and a non-ASCII word (the
existing test at `text_geometry.rs` covers the `日本語` case).

### WP8 — Prove the rendering is unchanged, and the counters bite

Intent: the primary acceptance property.

Required changes: a render comparison over a fixture set — a short file, a long
line clipped horizontally, a line with a multi-byte character at the clip, a
selection, a cursor on the last line, a document with semantic tokens overriding
a scan style, a document with the truncation suffix, and a horizontally scrolled
view. Then perturb the clamp cache (drop `revision` from the key) and confirm the
comparison fails.

Acceptance evidence: identical buffers; a failing comparison under the
perturbation; the perturbation reverted and recorded.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A clamp cache that survives a document change lets the view scroll
  past content, and a prefix check seeded wrongly misses a change above the edit.
  Both are visible defects; both are stop conditions.
- **Cancellation.** Closing the editor drops the cache. A file switch drops it
  (the path is part of the highlight key already; the clamp cache must include
  the document identity or the revision).
- **Restart.** Nothing durable.
- **Contention.** None.
- **A reload from disk.** The document's revision moves, so both caches drop.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. `head_cells` and
`cursor_cell` gain a parameter; both are private to `components/editor.rs`.
`TextSource` gains a method with a default implementation, or the snapshot adapter
gains one — the implementer must not break the trait's other implementors.

## 10. Required tests

### Focused unit tests

- A row whose line is longer than the window walks only to the window's end.
- The clip is unchanged for a character straddling the boundary.
- The clamp cache rebuilds on a revision change, a scroll, and a resize, and not
  otherwise.
- The token scan visits only the row's tokens.
- The style sweep produces the same style per segment as the previous `find`.
- An edit at line 4,900 does not re-read lines 1–4,899.
- An edit **above** the cached scan is detected.
- An edit **below** the cached scan is detected.
- `ScanKey` allocates nothing on a hit.
- `head_cells` and `cursor_cell` use the passed cursor line.

### Integration tests

- A frame over a 40-row viewport asserts the four counters, driven through
  `tests/tui_render`.

### Restart and recovery tests

None required.

### Contention and cancellation tests

None. Single-threaded.

### Security and negative tests

- An edit above the scan is detected (the invalidation's security-relevant
  direction).
- A keyed-hash collision does not silently keep stale spans — i.e. the
  comparison is on the hash's value, and the test does not rely on collision
  resistance for correctness.

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` for the counts; `editor_highlight.rs`'s own
unit tests for the prefix matrix; `components/editor.rs`'s own tests for the clip.
Every new assertion must be shown failing against the baseline.

## 11. Required verification commands

```bash
# The editor's own tests.
cargo test -p codegg --lib -- tui::components::editor tui::editor \
    tui::editor_highlight tui::text_geometry

# The cost member and the family.
cargo test --test tui_render -- tui_render_cost
cargo test --test tui_render

# The editor's static guards — all of them must stay green with no allowlist edit.
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_leaf_authority.py
python3 scripts/check_tui_editor_doc_drift.py

# The render-cost shape guard, and the proof it bites.
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_cost.py --self-test

# Rust-aware test inventory.
python3 scripts/rust_test_scan.py --validate

# Scoped, then the full sweep before closure.
scripts/verify.sh scoped HEAD~1
bash scripts/verify.sh full
```

## 12. Documentation updates

- `architecture/tui.md` §"The render path's caches (C006)": the clamp cache and
  the seeded prefix check.
- `architecture/tui.md` §"Bounded rendering and tree scheduling (C002)": confirm
  the bound still holds, now tighter.
- No `docs/tui.md` change: no key moves, and
  `scripts/check_tui_editor_doc_drift.py` must stay green without one.

## 13. Acceptance criteria

1. A visible row's read and walk are bounded by the drawn columns plus one
   character, not by the line's length.
2. The viewport resolver performs **zero** visible-window reads on a frame where
   the revision, the scroll position and the width are unchanged.
3. The semantic-token scan is `O(log tokens + tokens on the row)`, not
   `O(all tokens)` per row.
4. The per-segment style lookup is a single sweep, not a `find` from index 0.
5. An edit near the end of a 5,000-line document does not re-read the whole
   prefix, and an edit above the scan is still detected.
6. `ScanKey` allocates nothing on a hit, and `head_cells`/`cursor_cell` do not
   recompute the cursor line.
7. A word motion does not allocate per byte.
8. The rendered output is byte-identical to the baseline for the fixture set, and
   the comparison is demonstrated capable of failing.
9. ADR-0011 holds and every editor guard is green with no allowlist change.

## 14. Stop conditions

Stop and report rather than improvise if:

- A change would introduce a second text buffer, a materialized `Vec<Line>`, a
  cached document copy, or any state `scripts/check_tui_editor_text_authority.py`
  rejects. ADR-0011 is not reopened by a cost milestone.
- The clamp cache would have to retain document bytes rather than an integer.
- The prefix check cannot be seeded from the edit without missing a change above
  it. A missed change is a wrong colour, which is a correctness defect here
  because the cache is the only source of a row's spans.
- The per-row read cannot be bounded without changing the clip for a straddling
  multi-byte character or an escape sequence.
- The change would touch the key dispatch, the prefix grammar, the leaf tables,
  the completion popup, or the LSP request path.
- The change would need `src/tui/wrap.rs` (that is M006).
- A guard needs an allowlist entry to pass.

## 15. Closure evidence required

- The baseline and after counts for all four counters, per fixture, with the
  document sizes used.
- The prefix matrix's results — edit above, below, and near the end — and the
  perturbation shown failing.
- The render comparison's result and its demonstrated ability to detect a
  difference.
- The five editor guards' output, showing no allowlist change.
- The confirmation that ADR-0011 holds: no second buffer, no materialized rows.
- `cargo test --test tui_render`, the editor's unit tests, and
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI.
- The registry update and the unblock audit.

## 16. Handoff notes

- **The character loop has no early exit.** That is the whole of §3.1: the row is
  drawn from a window, but the walk continues to the end of the line.
- **The visible window is read twice per frame** — once by
  `longest_visible_width` for the clamp, once by `line_spans` for the rows. The
  first is the one to cache.
- **The token vector is already sorted.** `partition_point` is the fix, and the
  file already uses the pattern for search matches.
- **`syntax_ranges` and the boundaries are both ascending.** One index, one
  sweep. `find` inside the loop is the defect.
- **`first_changed_line` starts at 0 by construction.** C006 fixed the *key*; it
  did not fix the *scan*. The presentation knows where the edit was.
- **`ScanKey` allocates two `String`s per visible row.** Compare first.
- **`byte_at` is the keystroke path, not the frame path.** It is in scope because
  the fix is local, not because it is per-frame.
- **Do not touch `syntect`'s replay.** Its state cannot be checkpointed; only the
  re-read and the prefix length can be reduced.
