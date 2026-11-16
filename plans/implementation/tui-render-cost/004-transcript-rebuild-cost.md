# TUI Render Cost Milestone M004 — The Transcript's Rebuild Cost

Status: ready for handoff

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M004)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "The render path's caches (C006)", "The disclosure
  model: collapsed by default", and "The transcript's retention"

Applicable ADRs: **none required.**

Primary class: **polish** (cost)

## 1. Objective

A streaming tick copies the tail's rows **once** instead of twice, a rebuilt
message copies its parts' cached rows **once** instead of twice, a collapsed
tool call does not parse its payload, and the code-block wrapper emits one span
per style run rather than one per character.

## 2. Why this milestone is ready

No hard dependency. C006 built the caches this milestone reads —
`StreamingTailCache` (block-by-block incremental parse),
`MessagesWidget::markdown_part_rows` (per-part cache), and the layout cache's
dirty-suffix rebuild. This milestone does not add a new derivation; it removes
copies and dead work **around** those caches, and it is therefore strictly
smaller and lower-risk than C006 was.

## 3. Current implementation evidence

Line numbers are at the baseline. Re-derive them.

### 3.1 The streaming tail re-assembles, and therefore re-clones, on the fast path

`src/tui/components/messages.rs:2947` `StreamingTailCache::render`:

```rust
// The tail did not change: the rows are already rendered.
if self.epoch == epoch && self.text_len == text.len() && !self.blocks.is_empty() {
    return self.assemble();                    // :2965
}
...
self.blocks.extend(blocks);
self.epoch = epoch;
return self.assemble();                        // :3004
```

and `assemble` (`:3022`) is `assemble_markdown_blocks(&self.blocks)`, which
(`:3117`) does `lines.extend(block.rows.iter().cloned())`.

So the branch whose comment says *"the rows are already rendered"* renders them
again: every block's every row is deep-cloned into a fresh `Vec<Line>` on every
call. The tail is the whole response received so far, so this is
**O(response length)** per token batch, on top of the incremental parse that
C006 already fixed.

The returned `Vec` is then moved into the message's rows and copied again by the
layout cache's `push_message`, so the same content is materialized at least
three times per rebuild.

### 3.2 A cached part's rows are cloned twice per rebuild

`src/tui/components/messages.rs:667`:

```rust
let mut cache = self.markdown_parts.borrow_mut();
if let Some(entry) = cache.get(&(msg_idx, part_idx)) {
    if entry.content_len == content.len() && entry.width == width {
        return entry.rows.clone();             // :677
    }
}
```

and the caller (`:2259`) `lines.extend(rendered)` — a second copy. The search
arm at `:2250` is worse: `highlight_match_in_rendered` returns
`rendered.to_vec()` in every arm that does not find a first-line match, adding a
third copy.

During streaming the message being streamed into is rebuilt per token, and
`build_assistant_parts_lines` walks **every** part of it, so all of its
already-complete parts are re-cloned each time.

### 3.3 A collapsed tool call parses its payload

`src/tui/components/messages.rs:2372`:

```rust
let output_body: Option<Vec<String>> = if read_lang.is_some() {
    None
} else {
    Some(match render_structured_output_outline(name, payload) {
        Some(rows) => rows,
        None => format_payload_for_display(payload).lines().map(ToOwned::to_owned).collect(),
    })
};
```

This runs **before** the `else if *expanded && !output.is_empty()` gate at
`:2495`. `render_structured_output_outline` (`:3829`) does a full
`serde_json::from_str` and builds a tree of formatted `String`s, which is then
dropped for every collapsed call. The disclosure model makes collapsed the
default, so the common case is the one that pays.

### 3.4 The code-block wrapper allocates per character

`src/tui/components/messages.rs:3624` `wrap_code_spans`:

```rust
for span in spans {
    let style = span.style;
    for ch in span.content.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if col > 0 && col + ch_width > width {
            lines.push(Line::from(std::mem::take(&mut current)));
            col = 0;
        }
        current.push(Span::styled(ch.to_string(), style));   // :3641
        col += ch_width;
    }
}
```

One `Span` plus one heap `String` **per character**. An 80×100 code block is
~8,000 allocations and ~8,000 spans, which ratatui then resolves individually
when the paragraph is drawn. Called from `render_code_block` (`:3666`) and
`render_numbered_code_output` (`:3943`).

### 3.5 The numbered code output is copied twice before highlighting

`src/tui/components/messages.rs:3911`:

```rust
let code = output.lines().map(strip_read_line_number).collect::<Vec<_>>().join("\n");
let highlighted = highlight_code(&code, lang, theme.code_theme());
...
for (original, highlighted_line) in output.lines().zip(highlighted) {
```

The caller already did `output.lines().collect::<Vec<_>>().join("\n")` at
`:2534`, so the payload is materialized three times: once by the caller, once by
this strip-and-join, and once more by the highlighter's own line split. The
final `zip` then splits the original a third time.

### 3.6 `render_md_line_wrapped` measures, discards the spans, and re-wraps

`src/tui/components/messages.rs:4105` computes the total width of the styled
spans, returns the spans unchanged if they fit, and otherwise flattens them to a
plain `String` and re-wraps that — discarding every style. It is reachable only
from the Html and Table block renderers, so its impact is low, but it is the same
"derive twice" shape and it is in the same file.

## 4. Invariants that must not regress

- **The rendered output is byte-identical** for a given state, including the
  blank line between two Markdown blocks and the absence of one where the
  previous block already ended with one.
- **One producer of the transcript's rows.** `build_message_rows` still emits
  them; the layout cache is still their index; the scrollbar's row count and the
  drawn rows are still the same rows. This milestone removes copies, not
  producers.
- **The disclosure model is unchanged.** A collapsed call still shows the same
  header, the same hidden-output counter, and the same status row. Only the
  *body that is not drawn* stops being built.
- **The wrap's byte offsets and styles survive.** `wrap_code_spans` may change
  how many spans it emits; it may not change where a row breaks or what style a
  character carries.
- **The highlighter's output is unchanged.** `highlight_code` is not re-derived.
- **The streaming tail's cache still answers with the rows a fresh parse
  produces** — C006's equivalence property, asserted by
  `tui_render_cost::the_streaming_tail_renders_the_same_rows_as_a_fresh_parse`.

## 5. Scope

### In scope

- `StreamingTailCache::render` / `assemble` and the cached assembled rows.
- `MessagesWidget::markdown_part_rows` and its two call sites.
- `build_assistant_parts_lines`'s tool-call arm: the outline build's position.
- `wrap_code_spans`.
- `render_numbered_code_output` and its caller's pre-join.
- `render_md_line_wrapped`.
- Counters: `tail_row_copies`, `part_row_copies`, `collapsed_payload_parses`,
  `code_block_spans`.

### Explicitly out of scope

- `highlight_code` and the syntax highlighter's choice of engine.
- The Markdown parser (`parse_document`, `parse_markdown_blocks`), the block
  model, and the assembly rule.
- The layout cache's structure, its dirty-suffix rebuild, and its retention of a
  second rendered copy — C006's recorded decision.
- The transcript's retention and any cap.
- The disclosure model's behaviour: what expands, what the header says, what the
  hidden-output counter counts.
- `src/tui/wrap.rs`'s engines — M006. `wrap_code_spans` lives in `messages.rs`
  and is separate.
- The tool-output path's escape-atomicity (C006 §12; not this milestone).

## 6. Required production changes

### Core/domain

- `StreamingTailCache` must hold the **assembled** rows alongside its blocks, and
  `assemble()` must return a borrow rather than a fresh `Vec`. On a block change
  the assembled rows are rebuilt once; on the fast path they are returned as
  they are. `render` must then hand back a borrow (or an `Rc<[Line]>`) so the
  caller does not immediately copy it.
  If a borrowed return cannot be expressed because the caller needs ownership to
  feed `push_message`, the fallback is: keep the assembled rows cached and
  return `Rc<Vec<Line>>`, or restructure the caller so the tail's rows are
  pushed into the layout cache by reference. **A second clone is not a fallback.**
- `markdown_part_rows` must stop returning an owned clone on a hit. The caller
  consumes the rows immediately with `lines.extend(...)`, so the cache can hand
  back a borrow and the caller can extend from `&[Line]`. The search arm must
  stop producing a third copy: `highlight_match_in_rendered` should return
  either the borrowed rows untouched or the one modified line plus the rest by
  reference.
- The structured outline must be built **inside** the `expanded` gate. The
  collapsed header needs only the line count, which is available from
  `format_payload_for_display(payload).lines().count()` — or, better, from a
  cheap line count that does not format the payload at all. The implementer must
  confirm the header's hidden-output counter still shows the same number.
- `wrap_code_spans` must accumulate a contiguous run and emit one `Span` per
  style run per row, the way `chunk_word` in `wrap.rs` already does. The row
  break and the per-character widths are unchanged.
- `render_numbered_code_output` must take the stripped lines rather than
  re-deriving them, and the caller must stop pre-joining. The highlighter needs a
  `&str`; the implementer must build **one** owned buffer at the outermost point
  and pass slices from it, rather than three.
- `render_md_line_wrapped` must use the styled wrap (`wrap_styled_spans_by_word`)
  instead of flattening to a `String`, so the styles survive and the line is
  measured once. If the styled wrap's contract makes this a M006 dependency,
  record it and leave this item — it is the lowest-value item in the milestone.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. All state is per-widget, on the render thread.

### Frontend or operator surface

- Byte-identical rendering for the fixture set in §10.

### Security and authorization

No authorization boundary. One check: `render_structured_output_outline` reads
payloads that may be **externally framed** (`extract_trust_frame_payload_for_display`
runs first). Moving the build inside the `expanded` gate must not change what
`extract_trust_frame_payload_for_display` receives, and the trust frame must
still never be rendered as if it were the result. The payload is display-only;
the model path is untouched either way.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": the tail's assembled
  rows and the part cache's borrowed return, with their keys.
- `architecture/tui.md` §"The disclosure model: collapsed by default": note that
  a collapsed call builds no body, so the default is the cheap case.
- `src/tui/render_cost.rs`: the four counters above. `count_markdown_parse`
  already exists; `tail_row_copies` and `part_row_copies` are new and are the
  ones that make this milestone's claim enforceable.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter, beside the
  existing `the_streaming_parse_count_does_not_grow_with_the_response` and
  `a_message_parts_markdown_is_parsed_once`.
- `scripts/check_tui_render_cost.py`: a shape check that `assemble` is not
  called from the fast path without the cached rows, and that the outline build
  is inside the `expanded` gate. Add each to `--self-test`.

## 7. Ordered work packages

### WP1 — Baseline counts

Intent: a before for every claim.

Required changes: add the four counters with no behaviour change; run the new
assertions against the unmodified tree and record the failures.

Acceptance evidence: baseline counts for a streamed response at 1, 10 and 100
text deltas; a message with 1, 5 and 50 parts; a collapsed and an expanded
structured-output call; a code block of 20×40 and of 100×80.

### WP2 — Cache the tail's assembled rows

Intent: the fast path must not re-render what it says is already rendered.

Required changes: hold the assembled rows; return a borrow or a shared handle;
rebuild once per block change.

Acceptance evidence: `tail_row_copies` is 0 on the fast path and 1 per block
change; `the_streaming_tail_renders_the_same_rows_as_a_fresh_parse` still passes;
`the_streaming_parse_count_does_not_grow_with_the_response` still passes.

### WP3 — Stop the double copy of a part's rows

Intent: two copies per part per rebuild.

Required changes: borrowed return from `markdown_part_rows`; one extend from a
slice; the search arm stops adding a copy.

Acceptance evidence: `part_row_copies` is 1 per rebuilt part, down from 2 (3
under search); the rendered buffers are identical with and without a search
match.

### WP4 — Build the outline only when it is shown

Intent: the collapsed default must be the cheap case.

Required changes: move the build inside the `expanded` gate; keep the header's
counter identical.

Acceptance evidence: `collapsed_payload_parses` is 0 for a collapsed call and 1
for an expanded one; the collapsed header's rendered row is identical, including
the hidden-output count.

### WP5 — One span per style run in the code-block wrapper

Intent: 8,000 allocations for an 80×100 block.

Required changes: accumulate a run; emit one span per run per row.

Acceptance evidence: `code_block_spans` is bounded by `rows × style runs` rather
than by characters; the rendered buffer is identical for the fixture set,
including a block with a multi-byte character and one with a line longer than
the wrap width.

### WP6 — Copy the numbered output once

Intent: three materializations of the same payload.

Required changes: one owned buffer at the outermost point; slices to the
highlighter and the renderer.

Acceptance evidence: the rendered buffer is identical; the peak copy count is
recorded (there is no counter for this — record it in the closure evidence by
inspection of the change, or add a counter if one can be placed honestly).

### WP7 — Prove the rendering is unchanged, and the counters bite

Intent: the primary acceptance property.

Required changes: a render comparison over a fixture set — a paragraph, a
heading, a list, a table, a code block with and without a language, a block with
an OSC-8 hyperlink inside a code span, a multi-byte block, a collapsed tool
call, an expanded tool call with a JSON payload, a `read` output with a line
gutter, and a streaming tail at several lengths. Then perturb the tail's cached
assembly (e.g. return the cached rows while the blocks changed) and confirm the
comparison fails.

Acceptance evidence: identical buffers; a failing comparison under the
perturbation; the perturbation reverted and recorded.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A stale assembled tail shows text that is no longer there — the
  exact defect C006's closure record warned about when it made
  `streaming_tokens` private. The cache's key is the block set's identity; a
  block change must drop the assembled rows.
- **Cancellation.** A new response (a new epoch) clears the cache. A session
  teardown clears the widget.
- **Restart.** Nothing durable.
- **Contention.** None.
- **A tool result that lands on an already-drawn message.** That is a
  non-append change to the part's content; `content_len` moves, so the part
  cache misses. The implementer must confirm the length-key is still sufficient
  — a same-length replacement would hit the cache. C006 accepted the length key
  on the argument that parts are appended and never rewritten; if that argument
  does not hold for tool results, that is a finding to record, not to fix here.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. Internal
signatures in `src/tui/components/messages.rs` change; every caller is migrated
in the same change.

## 10. Required tests

### Focused unit tests

- The fast path returns the cached assembly and performs no copy.
- A block change rebuilds the assembly once.
- A new epoch clears the cache.
- A part cache hit performs one copy; a miss performs one parse and one copy.
- A search match inside a part does not add a third copy.
- A collapsed structured-output call performs no payload parse.
- An expanded one performs exactly one.
- `wrap_code_spans` emits one span per style run per row, and the row breaks are
  unchanged.
- `render_numbered_code_output` produces identical rows from the pre-stripped
  input.

### Integration tests

- The existing `tui_render_cost` members must keep passing unchanged; a new
  member or new assertions cover the four new counters.

### Restart and recovery tests

None required.

### Contention and cancellation tests

None. Single-threaded, per-widget.

### Security and negative tests

- A framed external payload still renders its payload inside the frame, and the
  trust banner is still never rendered as the result — in both the collapsed and
  the expanded state.
- A same-length replacement of a part's content is either detected or recorded
  as a known limitation of the length key.

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` is the home; the file already holds the
streaming-tail and part-cache equivalences, so the new assertions belong beside
them. Every new assertion must be shown failing against the baseline.

## 11. Required verification commands

```bash
# The messages component's own tests.
cargo test -p codegg --lib -- tui::components::messages

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

- `architecture/tui.md` §"The render path's caches (C006)": the tail's assembled
  rows and the part cache's borrowed return.
- `architecture/tui.md` §"The disclosure model: collapsed by default": the
  collapsed call builds no body.
- The closure record must carry the length-key finding from §8 as a recorded
  limitation or a follow-up, whichever the implementer establishes.

## 13. Acceptance criteria

1. The streaming tail's fast path performs **zero** row copies, down from a full
   re-assembly of every block's rows per token batch.
2. A rebuilt part performs **one** row copy, down from two (three under search).
3. A collapsed tool call performs **zero** payload parses; an expanded one
   performs one.
4. The code-block wrapper's span count is bounded by rows × style runs, down
   from one span per character.
5. The numbered `read` output is materialized **once**, down from three times.
6. The rendered output is byte-identical to the baseline for the fixture set,
   and the comparison is demonstrated capable of failing.
7. The disclosure model, the assembly rule, the highlighter's output, and the
   layout cache's role as the row index are unchanged.

## 14. Stop conditions

Stop and report rather than improvise if:

- A borrowed return cannot be expressed without a second clone **and** the
  caller cannot be restructured to consume a borrow. Do not settle for a
  different copy; report the shape.
- The assembled rows' invalidation cannot be keyed on the block set's identity.
  A stale tail is the defect this cache exists to avoid.
- Moving the outline build would change the collapsed header's count, or would
  move `extract_trust_frame_payload_for_display`'s input.
- `wrap_code_spans`'s row breaks would change for any fixture.
- The change would touch `highlight_code`, `parse_document`,
  `parse_markdown_blocks`, the assembly rule, or the layout cache's structure.
- The change would require touching `src/tui/wrap.rs` (that is M006) or the
  search path (C006 §12 says no).
- The part cache's length key turns out to be insufficient for tool results and
  fixing it would change the cache's identity. Record the finding; do not widen
  the milestone.

## 15. Closure evidence required

- The baseline and after counts for all four counters, per scenario, with the
  sizes used.
- The render comparison's result and its demonstrated ability to detect a
  difference.
- The confirmation that `the_streaming_tail_renders_the_same_rows_as_a_fresh_parse`
  and `the_streaming_parse_count_does_not_grow_with_the_response` still pass.
- The peak copy count for the numbered output, by inspection.
- The length-key finding, recorded.
- `cargo test --test tui_render`, the messages unit tests, and
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI.
- The registry update and the unblock audit.

## 16. Handoff notes

- **The fast path is the bug.** Read the comment at `:2963` and the line under
  it: the branch says the rows are already rendered and then renders them.
- **`markdown_part_rows` returns a clone on a hit.** That is the second half of
  the same defect, and it is paid once per part per rebuild.
- **The outline is built before the gate.** The disclosure model makes collapsed
  the default, so the common case pays for a JSON parse it never shows.
- **`wrap_code_spans` is per character, not per style run.** `chunk_word` in
  `wrap.rs` already shows the right shape; copy it rather than inventing one.
- **Three materializations of the numbered output.** Trace from `:2534` through
  `:3917` before changing either.
- **Do not touch the assembly rule.** It is what makes a cached tail equal a
  fresh parse, and C006's equivalence test asserts it.
- **`render_md_line_wrapped` is the lowest-value item.** If it turns out to need
  M006's styled wrap, drop it from this milestone and say so.
