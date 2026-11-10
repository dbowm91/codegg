# TUI Render Cost Milestone M006 — The Shared Wrap Engines' Internal Cost

Status: ready for handoff (lowest priority in this roadmap)

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M006)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "The styled, word-aware wrap", "Text wrapping", and
  "The render path's caches (C006)"

Applicable ADRs: **none required, and none may be created by this milestone.**

Primary class: **polish** (cost)

## 1. Objective

The styled wrap stops rescanning its span list from the front for every word and
stops measuring each word twice, and the plain engine stops allocating a `Vec` and
two `String`s per word — with the rendered rows, the byte offsets, the styles and
the escape handling all unchanged.

## 2. Why this milestone is ready

No hard dependency. C006 established that a cost change may not alter the wrap's
contract: it made two items `pub(crate)` and touched nothing else, and it recorded
the resulting acceptance criterion as **partly met** precisely because the wrap
primitive's own input copy survives:

> The **wrap's own** input copy remains: `wrap_styled_spans_by_word_rows` takes
> `&[Span<'static>]`, so the text is copied once per wrap. It now happens once
> per **content change** rather than once per frame, and removing it entirely
> would change the wrap primitive's signature — the `tui-composer-text-layout`
> line's contract, which §5 puts out of scope.

This milestone is the narrower, admissible successor: it changes **internal**
work inside `build_items`, `split_at_spans`, `style_at` and the plain engine, and
touches no signature, no output, and no escape rule.

It is last in the roadmap's sequencing because its benefit is now
per-content-change rather than per-frame: C006 already removed the per-frame
re-wrap. It is still worth doing — the composer pays it on every keystroke, and
the transcript pays it on every rebuild — but it is the one milestone that may
legitimately be deferred without leaving a per-frame cost in place.

## 3. Current implementation evidence

Line numbers are at the baseline. Re-derive them.

### 3.1 The styled wrap rescans its span list for every word

`src/tui/wrap.rs:455` `build_items`, in the `SegmentKind::Word` arm (`:500`):

```rust
SegmentKind::Word => {
    for (start, end, style) in split_at_spans(&source.spans, segment.start, segment.end) {
        let text = &source.text[start..end];
        pieces.push(Piece { text, style, start, end, is_escape: false, width: visible_width(text) });
    }
}
```

and `split_at_spans` (`:521`):

```rust
fn split_at_spans(spans: &[(usize, usize, Style)], start: usize, end: usize)
    -> Vec<(usize, usize, Style)>
{
    let mut parts = Vec::new();
    let mut cursor = start;
    for &(span_start, span_end, style) in spans {
        if span_end <= cursor { continue; }
        ...
```

The segments are already in source order, so `spans` is rescanned from index 0 for
every word. The cost is `O(words × spans)` per wrap, and the function allocates a
`Vec` per word. A paragraph of many inline-code or link spans is quadratic.

`StyledSource::style_at` (`:301`) has the same shape: a linear rescan per
space/escape segment.

### 3.2 Every word is measured twice

`src/tui/wrap.rs:591`, in `chunk_word` (`:557`):

```rust
if seg_start < piece.text.len() {
    let segment_width = visible_width(&piece.text[seg_start..]);
```

`build_items` already set `piece.width = visible_width(text)` at `:508`. In the
no-break case `seg_start == 0`, so the identical substring is measured again.
`visible_width` (`:190`) is a byte loop with a `unicode_width` call per character,
so this doubles the width work on the common path. The freshly split fragments at
`:579` are measured a second time too, having just been walked for their widths.

### 3.3 The plain engine allocates per word

`src/tui/wrap.rs:30` `wrap_to_strings`:

```rust
for (idx, chunk) in chunks.iter().enumerate() {
    if idx + 1 == chunks.len() {
        current.push_str(chunk);
        *current_width += chunk.chars()
            .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0))
            .sum::<usize>();
    } else {
        wrapped.push(chunk.clone());
```

`hard_break_word` is called for every word, so each word costs a `Vec` and one to
two `String`s even when it fits; `word_width` is then recomputed with a second
`chars().map().sum()`, and every non-final chunk is cloned.

This is the engine behind `wrap_count` (`:16`, the row estimate) and the
tool-output lines. **It is also the engine C006 §12 records as lacking
escape-atomicity on the tool-output path** — see §4.

### 3.4 The bound on this milestone

`wrap_styled_spans_by_word_rows` (`:633`) and `wrap_styled_spans_by_word`
(`:717`) are the composer's and the main buffer's primitives. Their signatures,
their output and their escape handling are owned by
`tui-composer-text-layout` and `tui-main-buffer-text-rendering`, both
**conditionally closed**. This milestone may not change any of the three.

## 4. Invariants that must not regress

- **The rendered rows are identical**, byte for byte, including where a row
  breaks, how a word is split when it exceeds the width, and the leading/trailing
  space handling.
- **The byte offsets are identical.** `Piece::start`/`Piece::end` are absolute and
  downstream code (the composer's cell↔byte mapping, the editor's `line_slice`)
  depends on them.
- **The styles are identical.** Every character keeps the span style it had.
- **The escape handling is identical, and the OSC-8 hyperlink survives whole.**
  The sequence's bytes are on no cell in either engine, and a naive word wrap
  would break the target at an arbitrary character. This is the hazard C006
  named; it must stay closed.
- **The plain engine's escape behaviour is unchanged — including its known lack
  of escape-atomicity on the tool-output path.** This milestone makes that engine
  cheaper; it does **not** fix, move, or mask the recorded defect. Fixing it is a
  behavioural change owned by the text-rendering line.
- **The search path is untouched.** C006 §12 records that the `/search` path is
  where two further defects live. `wrap_to_strings`'s behaviour on that path must
  be bit-identical.
- **No signature changes.** `wrap_styled_spans_by_word_rows`,
  `wrap_styled_spans_by_word`, `wrap_to_strings`, `wrap_count`,
  `visible_width`, `escape_sequence_len` and `elide_marked` keep their
  signatures and their `pub`/`pub(crate)` visibility.

## 5. Scope

### In scope

- `build_items`' span lookup (`split_at_spans` and `style_at`).
- `chunk_word`'s double width measurement.
- `wrap_to_strings`'s per-word allocation.
- Counters: `wrap_span_rescans`, `wrap_width_measures`.

### Explicitly out of scope

- Any signature, visibility, or output change.
- The escape parser (`escape_sequence_len`, `scan_to_terminator`,
  `advance_past_escape`) and the `ESC` constant.
- The plain engine's escape-atomicity defect, and the `/search` path's two
  defects. Do not fix, move, or work around them.
- `wrap_code_spans` in `src/tui/components/messages.rs` — M004.
- The composer's layout cache and the transcript's part cache.
- The visual-row geometry and the cell↔byte mapping.
- Anything outside `src/tui/wrap.rs`.

## 6. Required production changes

### Core/domain

- `build_items` must carry a monotonically advancing span cursor (or use
  `partition_point` on `spans`) so that `split_at_spans` and `style_at` cost
  `O(spans + words)` in total rather than `O(words × spans)`. The cursor must
  advance, never restart: segments are produced in source order.
- `split_at_spans` must stop allocating a `Vec` per word. The implementer must
  either inline the split into `build_items`' loop or have the caller pass a
  reusable buffer.
- `chunk_word` must use `piece.width` when `seg_start == 0`, and must accumulate
  the fragment widths it already walks rather than re-measuring each fragment
  with `visible_width`.
- `wrap_to_strings` must return chunk **byte offsets** (or ranges) and let the
  caller push slices of the input, accumulating width during the single existing
  character loop. If returning offsets would change a signature, the implementer
  must instead avoid the `String` clone per chunk by moving the chunk rather than
  cloning it, and must record that the allocation remains.
- No function may gain or lose a parameter, a return type, or a visibility
  modifier.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. Pure functions.

### Frontend or operator surface

- Byte-identical rendering for the fixture set in §10. This is the milestone's
  entire acceptance property.

### Security and authorization

None. One note: the wrap is where the OSC-8 hyperlink is kept atomic, and that
atomicity is a display-integrity property (a broken target renders as text the
user did not write). The fixture set must include an OSC-8 hyperlink and the
comparison must cover it.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": record that the wrap's
  internal span work is linear, and that the input copy remains (C006's partly-met
  criterion is unchanged by this milestone).
- `architecture/tui.md` §"The styled, word-aware wrap": confirm the contract is
  unchanged; do not restate the algorithm.
- `src/tui/render_cost.rs`: the two counters.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter.
- `scripts/check_tui_render_cost.py`: a shape check that `split_at_spans` is not
  called from inside a per-word loop without an advancing cursor, and that the
  escape parser and `ESC` are untouched. Add each to `--self-test`.

## 7. Ordered work packages

### WP1 — Baseline counts

Intent: a before for both claims.

Required changes: add the two counters with no behaviour change; run the new
assertions against the unmodified tree and record the failures.

Acceptance evidence: baseline counts for a paragraph of one span, of twenty
inline spans, of a hundred spans; a long word that must be hard-broken; a
paragraph with an OSC-8 hyperlink.

### WP2 — Advance the span cursor

Intent: `O(words × spans)` becomes `O(spans + words)`.

Required changes: the advancing cursor in `build_items`, `split_at_spans` and
`style_at`; no per-word `Vec`.

Acceptance evidence: `wrap_span_rescans` is bounded by `spans + words`, down from
`words × spans`; the rendered rows and byte offsets are identical for every
fixture.

### WP3 — Measure each word once

Intent: remove the second `visible_width` on the common path.

Required changes: use `piece.width` when `seg_start == 0`; accumulate the
fragment widths the loop already computes.

Acceptance evidence: `wrap_width_measures` is one per word, down from two (three
for a hard-broken word); the rendered rows are identical for a fixture with a
wide character, a combining character, and an escape sequence inside a word.

### WP4 — Stop the plain engine's per-word allocation

Intent: `wrap_to_strings` allocates a `Vec` and one to two `String`s per word.

Required changes: as §6 describes. If the signature must change, do not change it
— take the reduced form and record the remainder.

Acceptance evidence: the returned `Vec<String>` is byte-identical for every
fixture; the allocation reduction is recorded by inspection, since there is no
honest counter for "allocations" that this milestone can place without measuring
the allocator.

### WP5 — Prove the rendering is unchanged, and the counters bite

Intent: the primary acceptance property, and the escape hazard.

Required changes: a render comparison over a fixture set — an empty string, a
single word, a paragraph of plain text, a paragraph with inline code and bold,
a paragraph of a hundred spans, a word longer than the width (hard break), a
leading and a trailing space, a double space, a tab, a wide (CJK) character, a
combining character, a CSI colour sequence, and an **OSC-8 hyperlink** — for
both `wrap_styled_spans_by_word_rows` and `wrap_to_strings`. Then perturb the
advancing cursor (restart it per word) and confirm the comparison still passes —
it should, because the cursor is a cost change — and instead perturb a boundary
(e.g. drop the `style_end <= cursor` guard) and confirm the comparison **fails**.

Acceptance evidence: identical rows, offsets and styles for the unperturbed tree;
a failing comparison under the boundary perturbation; both perturbations
reverted and recorded.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A wrongly advancing cursor mis-styles or mis-splits a word. The
  comparison catches it because it compares styles and offsets, not just text.
- **Cancellation.** None; pure functions.
- **Restart.** None.
- **Contention.** None.
- **A segment that is not in source order.** The cursor advance depends on
  ordering. If `tokenize` (`:356`) can emit an out-of-order segment, the
  implementer must either prove it cannot or fall back to a binary search rather
  than a monotone cursor. This is a stop condition if neither holds.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. No signature,
visibility, or public API change anywhere in `src/tui/wrap.rs`.

## 10. Required tests

### Focused unit tests

- The span work is bounded by `spans + words`, not `words × spans`.
- Each word is measured once.
- The rendered rows are identical for the fixture set.
- The byte offsets are identical for the fixture set.
- The styles are identical for the fixture set.
- An OSC-8 hyperlink survives whole and is not broken at a word boundary.
- `wrap_to_strings`'s output is identical for every fixture, including a body
  with an escape sequence (the known non-atomic case must behave exactly as it
  does today).

### Integration tests

- A frame drawing the composer and a frame drawing the transcript assert the two
  counters, driven through `tests/tui_render`.

### Restart and recovery tests

None required.

### Contention and cancellation tests

None. Pure functions.

### Security and negative tests

- The OSC-8 hyperlink fixture: the target appears once per rendered row it
  occupies and is never split.

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` for the counts; `src/tui/wrap.rs`'s own
tests (it already has `visible_width_ignores_escape_sequences` and others) for the
fixture set. Every new assertion must be shown failing against the baseline, and
the boundary perturbation must be shown failing — a comparison that cannot detect
a boundary change is not evidence for this milestone.

## 11. Required verification commands

```bash
# The wrap engine's own tests.
cargo test -p codegg --lib -- tui::wrap

# The cost member and the family.
cargo test --test tui_render -- tui_render_cost
cargo test --test tui_render

# The shape guard, and the proof it bites.
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_cost.py --self-test

# Rust-aware test inventory.
python3 scripts/rust_test_scan.py --validate

# Scoped, then the full sweep before closure.
scripts/verify.sh scoped HEAD~1
bash scripts/verify.sh full
```

## 12. Documentation updates

- `architecture/tui.md` §"The render path's caches (C006)": the wrap's internal
  span work is linear; the input copy remains, so C006's partly-met criterion is
  unchanged.
- `architecture/tui.md` §"The styled, word-aware wrap": a one-line confirmation
  that the contract (styles, byte offsets, escape atomicity) is unchanged. Do not
  restate the algorithm.

## 13. Acceptance criteria

1. The styled wrap's span work is `O(spans + words)`, down from
   `O(words × spans)`.
2. Each word is width-measured once, down from twice (three for a hard-broken
   word).
3. The plain engine no longer allocates a `Vec` and two `String`s per word, or the
   remainder is recorded with the reason.
4. The rendered rows, the byte offsets, the styles and the escape handling are
   identical for the fixture set, including the OSC-8 hyperlink.
5. No signature, visibility, output, or escape rule changed.
6. The plain engine's known lack of escape-atomicity on the tool-output path is
   neither fixed nor moved, and the `/search` path's behaviour is bit-identical.
7. The comparison is demonstrated capable of failing under a boundary
   perturbation.

## 14. Stop conditions

Stop and report rather than improvise if:

- A change would need a signature, visibility, or output change. That reopens
  `tui-composer-text-layout` or `tui-main-buffer-text-rendering`.
- A change would alter the escape handling, the OSC-8 atomicity, or the plain
  engine's known non-atomic behaviour on the tool-output path.
- A change would touch the `/search` path.
- `tokenize` can emit out-of-order segments and a monotone cursor is therefore
  unsound, and a binary search would change the complexity beyond the milestone's
  value. Report and stop.
- The fixture set cannot be made to detect a boundary change.
- The change would reach outside `src/tui/wrap.rs`.

## 15. Closure evidence required

- The baseline and after counts for both counters, per fixture.
- The render comparison's result — rows, byte offsets and styles — and its
  demonstrated ability to fail under a boundary perturbation.
- The OSC-8 hyperlink fixture's result.
- The confirmation that no signature, visibility, output or escape rule changed,
  verified against the diff.
- The statement of what remains of the plain engine's per-word allocation, if
  anything, and why.
- `cargo test --test tui_render`, the wrap unit tests, and
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI.
- The registry update and the unblock audit.

## 16. Handoff notes

- **This milestone may not change the wrap's contract.** C006's closure record
  spells out why: the composer's primitive is owned by a conditionally-closed
  line. Read `plans/closure/repository-audit-corrective/006-status.md` §5 before
  touching the file.
- **`chunk_word` already measures the fragment.** The loop walks every character
  for `ch_width`; summing it costs nothing and re-measuring costs a second pass.
- **`split_at_spans` allocates a `Vec` per word.** Inlining it into the caller's
  loop is the fix; a reusable buffer is the fallback.
- **The segments are in source order** — that is what makes a monotone cursor
  sound. Prove it from `tokenize` before relying on it.
- **The plain engine's escape bug is not yours.** Making the engine cheaper is;
  making it atomic is not.
- **Perturb a boundary, not the cursor, to prove the comparison.** A cursor
  perturbation is a cost change and should not change the output; a boundary
  perturbation should, and if it does not, the comparison is not evidence.
- **This is the roadmap's deferrable milestone.** If budget is tight, closing
  M001–M005 without M006 leaves no per-frame cost behind.
