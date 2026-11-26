# TUI Render Cost Milestone 006 — Closure Status

Status: **closed** (2026-11-10). This is the roadmap's last milestone and its
**deferrable** one; it is closed rather than deferred because its budget held.

Implementation plan:
`plans/implementation/tui-render-cost/006-shared-wrap-engine-cost.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M006)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created.

## 1. Executive finding

The styled wrap's span lookup rescanned its span list **from the front for every
word**. `split_at_spans` iterated `spans` from index 0 for each word, so the cost
was `O(words × spans)` per wrap and it allocated a `Vec` per word — a paragraph of
many inline-code or link spans was quadratic. `StyledSource::style_at` had the same
shape for each space and escape segment.

On top of that, every word was **measured twice**: `build_items` set
`piece.width = visible_width(text)`, and `chunk_word` then re-measured the identical
substring on the no-break path (`seg_start == 0`), plus a second pass over each
freshly split fragment. `visible_width` is a byte loop with a `unicode_width` call
per character, so this doubled the width work on the common path.

The plain engine allocated a `Vec` and one to two `String`s per word, then
recomputed the word's width with a second `chars().map().sum()`.

## 2. The cost sites, before and after

| Fixture | Counter | Before | After |
|---|---|---|---|
| one span, 9 words (composer shape) | `wrap_span_rescans` | 17 | 17 (**flat** — one span has nothing to rescan) |
| 20 spans, 20 words | `wrap_span_rescans` | **400** | **58** |
| 100 spans, 100 words | `wrap_span_rescans` | **10,000** | **298** |
| long word `abcdefghij`, width 4 | `wrap_width_measures` | **4** | **1** |
| OSC-8 paragraph, 3 spans, 5 words | `wrap_width_measures` | **11** | **5** |
| cold composer frame, 9 words | `wrap_width_measures` | **18** | **9** |

The one-span row is reported as the flat case it is: the quadratic evidence is the
20- and 100-span rows. The styled engine's span work is now
`O(spans + queries)`, not `O(words × spans)`.

### 2.1 The captured baseline failures

```
twenty spans cost one examination each, not a rescan from the front for every
word: 400 examinations for 20 spans and 20 words
a hundred spans cost one examination each ...: 10000 examinations for 100 spans
a hard-broken word is measured once, where the chunker measured every fragment
again: 4 measurements
one measurement per word, not one per word plus one per piece again:
11 measurements for 5 words
the composer's frame measures each word once: 18 measurements for 9 words
```

## 3. The shape

- `SpanCursor` replaces `StyledSource::style_at` and `split_at_spans`. `build_items`
  resolves a word's parts and a separator's style through one advancing index; the
  per-word `Vec` is gone (the split is inlined and writes straight into `pieces`).
- `chunk_word` takes `piece.width` when `seg_start == 0` and accumulates the
  fragment widths from the walk it already performs.
- `wrap_to_strings` chunks a word by **byte range** into a reused buffer; the
  per-word `Vec` and the per-chunk `String` are gone.

**The ordering proof.** `tokenize`'s arms all advance `idx` strictly: the escape
arm sets `idx = advance_past_escape(..)` (≥ 1 byte), the newline arm `idx += 1`,
and the space and word arms walk at least one character before pushing, because
the character at `idx` is theirs by construction (the newline and `ESC` cases
returned earlier). So every segment is `(start, end)` with `start == previous.end`
and `start < end`: segments are contiguous, non-overlapping, in source order, and
cover the source. Every query `build_items` makes is therefore non-decreasing —
the precondition for one advancing index. A monotone cursor was used, not a binary
search, and `tokenize_emits_its_segments_in_source_order` asserts the property
directly over the fixture set, so the proof is executable rather than prose.

## 4. The two perturbations — both recorded and reverted

**The cursor perturbation is a cost change and must not alter output.** Restarting
the cursor per word (`let mut span_cursor = SpanCursor::new();` inside the `Word`
arm) left the equivalence tests green —

```
test tui::wrap::tests::the_plain_engine_matches_the_pre_m006_algorithm ... ok
test tui::wrap::tests::the_wrapped_rows_match_the_pre_m006_algorithm ... ok
```

— and correctly failed the cost member (`20 examinations for 5 words`, three tests
failed). That is the pair of observations that says the counter measures the thing
the comparison cannot see.

**A boundary perturbation must alter output.** Dropping the span end from a part's
boundary (`let end = segment.end.min(span_end);` → `let end = segment.end;`) failed
the comparison on the **style** of the second half of "international":

```
assertion `left == right` failed: a word split across two spans at width 1:
the rendered rows, their byte offsets or their styles changed
  left:  [ ... StyledSlice { text: "n", style: Style::new(),          source_start: 5, source_end: 6 } ... ]
 right: [ ... StyledSlice { text: "n", style: Style::new().red(),    source_start: 5, source_end: 6 } ... ]
```

**The both-sides proof.** The new comparison tests were also run against
`git show HEAD:src/tui/wrap.rs` plus only the new test section — the pre-change
engine — and all three passed. So the equality tests are guard rails that pass on
both sides by construction, not an artefact of the new code agreeing with itself.

## 5. Invariant review

- **The rendered rows are byte-identical**, including where a row breaks, how a
  word is split when it exceeds the width, and the leading/trailing space handling.
- **The byte offsets are identical.** `Piece::start`/`Piece::end` are absolute and
  the composer's cell↔byte mapping and the editor's `line_slice` depend on them.
- **The styles are identical**; the boundary perturbation above proves the
  comparison detects a style change.
- **The escape handling is identical and the OSC-8 hyperlink survives whole.**
  The fixture set includes the hyperlink and compares to the byte.
- **No signature, return type or visibility changed.** `wrap_styled_spans_by_word_rows`,
  `wrap_styled_spans_by_word`, `wrap_to_strings`, `wrap_count`, `visible_width`,
  `escape_sequence_len` and `elide_marked` are untouched.
- **`Piece`/`Word`/`Space`/`Item` gained `Debug`/`PartialEq`** — private types, for
  the comparison's `assert_eq!`. No public type changed.

## 6. Security review

None required, with one note the plan named: the wrap is where the OSC-8 hyperlink
is kept atomic, and that atomicity is a display-integrity property — a broken
target renders as text the user did not write. The fixture set includes the
hyperlink and the comparison covers it.

## 7. Migration and compatibility review

None. No protocol, storage, configuration or migration change; no signature,
visibility or public API change anywhere in `src/tui/wrap.rs`. The change does not
reach outside that file.

`wrap_to_strings` is also the engine behind the chat body wrap, whose rows M003
caches; its output staying byte-identical is what keeps `tui_chat_cost` green, and
that was used as a cross-check rather than treated as an obstacle.

## 8. Unresolved findings and deferrals

- **The plain engine's escape-atomicity defect is neither fixed nor moved.**
  `plans/closure/repository-audit-corrective/006-status.md` §12 records that
  `wrap_to_strings` lacks escape-atomicity on the tool-output path. This milestone
  made the engine cheaper; making it atomic is a behavioural change owned by the
  text-rendering line. A differential test covers a CSI and an OSC-8 body and
  compares to the byte, so the recorded behaviour is pinned.
- **What remains of the per-word allocation.** Gone: the per-word `Vec`, the
  per-chunk `String` in the chunker, and both redundant width passes. Remaining
  and irreducible without a signature change: **one `String` per output line**
  (`wrapped.push(..)` — that is the returned value) and one byte copy of the final
  chunk into the reused line. A word itself allocates nothing.
- **The styled engine still allocates one small `Vec` per word** for its `pieces`,
  obtained by `mem::take` in `flush_word`. That predates this milestone and is the
  item structure, not the span lookup; changing it would change `chunk_word`'s
  input type, so it was left alone.
- **The wrap's input copy remains** (`wrap_styled_spans_by_word_rows` takes
  `&[Span<'static>]`), so C006's partly-met criterion is **unchanged** by this
  milestone, as `architecture/tui.md` records.
- **The trailing run's double-count is fixed** in the corrective pass of
  2026-11-10. `chunk_word` added every character's width as it walked, then added
  the run's width again at the flush, so a word spanning two or more spans
  measured wider than it draws and broke early — which is why such a word broke
  differently from the plain engine. The fix is the missing second addition, and
  it **changes where rows break**, so it needed its own pass rather than riding
  on a cost milestone. `a_word_split_across_spans_breaks_at_its_true_width`
  pins it, and the reference model was corrected with it — a reference that
  reproduced the defect would have asserted it.
- **`render_md_line_wrapped` is not touched** — it is M004's deferred item, and
  routing it through the styled wrap would change rendered output.

## 9. Verification executed

```text
$ cargo fmt --check
exit 0
$ cargo test -p codegg --lib -- tui::wrap
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 7875 filtered out
$ cargo test -p codegg --lib -- tui::components::prompt tui::components::messages
test result: ok. 127 passed; 0 failed; 0 ignored; 0 measured; 7768 filtered out
$ cargo test --test tui_render -- tui_wrap_cost
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 1183 filtered out
$ cargo test --test tui_render -- tui_chat_cost
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 1168 filtered out
$ cargo test --test tui_render
test result: ok. 1189 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 34 injections, all rejected
$ python3 scripts/check_comment_budget.py
comment budget guard passed
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

`docs/comment-budget.toml` was **banked twice** during this milestone, explicitly,
because three files crossed their recorded ceilings and then `tui_wrap_cost.inc`
grew once more when the assertion bound was tightened. Only these rows moved: the
global triple, `src/tui/render_cost.rs` 227→256, `src/tui/wrap.rs` 207→276, and
`tests/tui_render/tui_wrap_cost.inc` added at 67.

The tree-level `scripts/verify.sh quick` result is recorded once, in the roadmap's
closure.

## 10. Roadmap disposition and registry update

M006 is **closed**. The roadmap named it the deferrable milestone: closing
M001–M005 without it would have left no per-frame cost behind, because C006 had
already removed the per-frame re-wrap. It landed anyway, and the benefit is now
per-content-change — the composer pays it on every keystroke and the transcript on
every rebuild.

The roadmap's milestone table records M006 as `closed` with this record linked;
`plans/registry.md` moves its row to the closed disposition. The unblock audit
found nothing to unblock.
