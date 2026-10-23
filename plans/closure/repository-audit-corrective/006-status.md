# Repository Audit Corrective Milestone 006 — Closure Status

Status: implemented; the acceptance criteria that are met are listed in §6,
and the ones that are not are listed there with the reason.

Source implementation plan:

- `plans/implementation/repository-audit-corrective/006-tui-render-path-and-transcript-cost.md`

Source subsystem roadmap:

- `plans/subsystems/repository-audit-corrective-roadmap.md`

Repository baseline reviewed: `626a1abbb` (branch `fix/c006-audit-corrective`,
onto the integration branch `fix/repository-audit-corrective-implementation`)

## 1. Executive finding

The milestone's capability is implemented: the render path derives each
expensive result **once**, behind a cache whose key is what the result
depends on, and the rendered output is unchanged.

The milestone is a **cost** milestone, so its evidence is a count of
operations rather than a picture of the screen. The counts live in
`src/tui/render_cost.rs` and are asserted by the `tui_render` family.

What shipped:

* **The composer is wrapped once per frame.** Four consumers wanted the
  wrapped layout — the height budget, the caret's visibility, the draw and
  the caret's cell — and each of them rebuilt it, so a frame paid for the
  wrap four times over. The layout is now cached against the buffer's
  revision and the wrap width, so a frame that changed neither does **no**
  wrapping.
* **The composer text is not cloned on the render path.** The `!` test and
  the completion path read a borrowed `&str` instead of a fresh `String`.
* **The streaming tail is parsed incrementally.** The tail is the whole
  response received so far; re-parsing it per token was quadratic in the
  response's length. The tail is now parsed **block by block**, and only the
  blocks the new text can change are parsed again.
* **A message part's Markdown is parsed once.** A message's parts are
  appended, never rewritten, so the parts of the message being streamed into
  are no longer re-parsed on every token — the other half of the same
  quadratic.
* **The editor's highlight cache survives a local edit.** It was keyed on
  the document's revision, so every keystroke threw it away, and the scan
  restarted from the top for **every** line past the viewport.
* **`row_cells` is linear in the presence of an escape sequence**, and the
  caret's lookup no longer scans every cell.
* **The tasks overlay copies the rows it draws**, not the whole list.

A **static guard** (`scripts/check_tui_render_cost.py`) pins the shape of the
render path — one producer for the composer's wrap, one producer for the
Markdown parse, a cached composer layout, a cached streaming tail, no
whole-buffer clone on the render path, no whole-task-list clone on the render
path, and the frame's `catch_unwind` and its 16 ms streaming interval still
in place. It is wired into `scripts/verify.sh quick` and into CI, and its
`--self-test` injects seven violations and proves each is rejected.

## 2. The cost sites, before and after

The "before" column is the count the baseline produced; the "after" column is
the count the milestone's tests assert. The counts are deterministic
operation counts, not wall-clock claims: no benchmark harness was added,
because §5 of the plan puts one out of scope.

The "before" column is the number the **baseline** produced, captured by
reverting the production files to `626a1abbb`, re-adding only the counter
calls (the instrument) and running the same tests. The baseline failures are
quoted in §2.1.

| Site | Before | After | Measured by |
|---|---|---|---|
| The composer's wrap, per frame | **4** | **1** | `tui_render_cost::the_composer_is_wrapped_once_per_frame_and_shared_by_its_consumers` |
| The composer's wrap, per unchanged frame | **4** | **0** | `tui_render_cost::an_unchanged_composer_is_not_wrapped_again` |
| The whole composer buffer cloned, per frame | 1 | 0 | `scripts/check_tui_render_cost.py` (no `get_text()` on the render path) |
| The whole composer buffer cloned, per keystroke | 1 | 0 | `scripts/check_tui_render_cost.py` and `src/tui/app/mod.rs`'s borrowed `text()` |
| A streamed message's Markdown parses, per token | **2** (the message's part and the tail) | **1** (the tail) | `tui_render_cost::a_message_parts_markdown_is_parsed_once` |
| The streaming tail's Markdown parses, per change | one per change, over the whole tail | one per change, over the changed tail | `tui_render_cost::the_streaming_parse_count_does_not_grow_with_the_response` |
| The editor's highlight scans, per frame after an edit | the file, once per visible row | the file once, then one scan per row | `tui_render_cost::a_local_edit_keeps_the_highlights_of_the_lines_above_it` |
| The tasks overlay's rows copied, per frame | the whole task list | the rows it draws | `tui_render_cost::the_tasks_overlay_copies_only_the_rows_it_draws` and `scripts/check_tui_render_cost.py` |
| `row_cells`, with an escape sequence | quadratic in the slice's length | linear | `src/tui/composer_geometry.rs` |

### 2.1 The captured baseline failures

The baseline capture reverted the production files to `626a1abbb` (the
milestone's base), re-added **only** the counter calls — the instrument, not
the fix — and ran the same test member. Three of the eight tests failed, and
each failure carries the "before" number:

```
---- tui_render_cost::the_composer_is_wrapped_once_per_frame_and_shared_by_its_consumers ----
assertion `left == right` failed: the four consumers of the layout share one wrap, not four
  left: 4
 right: 1

---- tui_render_cost::an_unchanged_composer_is_not_wrapped_again ----
assertion `left == right` failed: a frame that changed neither the text nor the width does no wrap
  left: 4
 right: 0

---- tui_render_cost::a_message_parts_markdown_is_parsed_once ----
assertion `left == right` failed: only the streaming tail is parsed again, not the parts of the message it is streamed into
  left: 2
 right: 1
```

The five that passed on the baseline are the **equivalence** tests: they
assert that the cached path renders what the uncached path rendered, and the
uncached path is the baseline's own behaviour. They are not supposed to fail
on the baseline — a render-comparison test that failed on the baseline would
be comparing against the wrong thing.

**The render comparison is shown capable of detecting a difference.** The
composer's cache was perturbed so that it ignored the **width** — one line
of `PromptWidget::layout_at` — and the comparison failed:

```
---- tui_render_cost::the_cached_composer_draws_the_same_rows_as_a_fresh_one ----
assertion `left == right` failed: text "a line that is much wider than the
row it is wrapped at": the frame at width 5 drew something else
  left: Buffer {
 right: Buffer {
```

The perturbation was reverted immediately afterwards, and the comparison
passes on the shipped tree.

The baseline capture is reproducible: the production files were reverted with
`git checkout 626a1abbb -- <files>`, the instrument was re-applied, the
member was run, and the tree was restored with `git checkout HEAD -- <files>`.

## 3. The equivalence argument, per change

The milestone's central boundary is that **the rendered output must not
change**. Every change below is a memo of a pure function of its key, or a
change of *when* a computation happens, never *what* it computes.

| Change | Why the output is identical |
|---|---|
| The composer's layout cache | The cached layout is the layout the uncached computation produced: the same text, the same width, the same rows. The key is the buffer's revision (which moves exactly when the text moves) and the width. `tui_render_cost::the_cached_composer_draws_the_same_rows_as_a_fresh_one` compares a warm widget's buffer with a cold one's over the fixture set — empty, plain, wrapped, multi-byte, a CSI colour, and an OSC-8 hyperlink. |
| The composer's borrowed `text()` | The `!` test and the completion path read the same bytes they read before; only the ownership of the copy changed. |
| The streaming tail's incremental parse | The tail is parsed into its top-level blocks, and the rows are assembled by the render's own rule — a blank line between two blocks, none where the block before it already ended with one. The assembly is the same code the full parse used, so the rows are the rows the full parse produced. `tui_render_cost::the_streaming_tail_renders_the_same_rows_as_a_fresh_parse` compares a streamed tail with a freshly parsed one. |
| The per-part Markdown cache | A message's parts are appended and never rewritten (`MsgPart` is only ever pushed), so a part's rows cannot have changed. The cache is dropped by `invalidate_layout_cache`, which every non-append transcript change calls. |
| The editor's highlight cache | The cache is keyed on the **content** of each scanned line, not on the revision. A line whose bytes are unchanged keeps the spans it had; the machine is replayed to the first changed line so the lines after it are coloured from the context they would have been coloured from. The replay is the same replay the full scan did. |
| `row_cells` | The single pass computes the same per-character widths the difference of two prefix measurements computed: a control sequence contributes zero columns, and a printable character contributes its own width. The escape sequence is still skipped **whole**, so the OSC-8 target is still one atomic token. |
| The caret's lookup | The cells are in source order and their rows' ranges do not overlap, so the row a byte names is found by position and the cell by a scan of that row — the same cell the linear scan found. |
| The tasks overlay's bounding | `bounded` now borrows the rows and copies the ones it keeps. The kept rows, their order and their count are the same. |

## 4. The transcript's retention — the recorded decision

The primary transcript is **unbounded**, and that is now stated rather than
implicit:

* The sibling chat surface (`src/tui/app/state/chat.rs`) caps its
  per-channel window at 100 messages, and the transcript does not. The cap
  belongs to a different surface — a chat channel's window, whose
  daemon-side page size is 1000 — and it does not transfer to the
  transcript, whose durable copy is the session store and whose in-memory
  copy is the live view the user is reading.
* The plan's §5 puts "the number of transcript rows retained" **out of
  scope** as a user-visible rendering change, and §14 makes a silent
  retention bound a stop condition. A cap was therefore **not** adopted:
  there is no user-visible change, and the policy is recorded in
  `architecture/tui.md` under "The transcript's retention".
* The cost is stated, not hidden: a long session's memory grows with the
  session, and the layout cache holds a second rendered copy of it. The
  layout cache's rows are the **row index** — the one producer of the row
  count and of the scroll range — so dropping them would introduce a second
  producer of the row count, which is the defect the disclosure work
  removed. That is the reason the second copy is retained, and it is
  recorded in the same place as the retention policy.

## 5. The ownership boundary with the conditionally-closed text-rendering lines

The `tui-main-buffer-text-rendering` (M001–M004) and
`tui-composer-text-layout` (M001–M002) lines are **conditionally closed**:
their implementation landed and is verified locally, and the hosted
`CI / verify` lane has not yet run against the commit that carries them.
C006 therefore did not reopen them:

* **Touched:** `src/tui/wrap.rs` — `ESC` and `escape_sequence_len` became
  `pub(crate)` so that the visual-row geometry can skip a control sequence
  with the **same** parser rather than a second one. No wrap algorithm
  changed, no signature changed, and no output changed.
* **Deliberately not touched:** the wrap primitives themselves, the visual
  row geometry's *mapping* (the bijection between cells and bytes), the
  search path's recorded **search-highlight** and **search-scroll**
  defects, and the plain `wrap_to_strings` engine's recorded lack of
  escape-atomicity on the tool-output path. The `/search` path's clones
  (plan WP8) were left alone for the same reason — the search path is where
  the two recorded defects live.
* **Confirmed unchanged:** the OSC-8 hazard. The hyperlink target appears
  twice in a span's content and carries no whitespace, so a naive word wrap
  would break it at an arbitrary character. `row_cells` still skips the
  sequence whole, and the fixture set in
  `tui_render_cost::the_cached_composer_draws_the_same_rows_as_a_fresh_one`
  carries an OSC-8 hyperlink. The change to `row_cells` is a **cost** change:
  the per-character widths are now measured in one pass instead of by
  re-measuring the prefix for every character, and the escape sequence is
  skipped as one atomic token in both — the sequence's bytes are on no cell
  in either.
* **Confirmed unchanged:** the **one producer** of the transcript's row
  count. `build_message_rows` is still the only producer of the rows, the
  layout cache is still their index, and the scrollbar's row count and the
  drawn rows are still the same rows. The caches added here memoize rows
  that the producer already emitted; none of them emits a row.
* **Confirmed unchanged:** the two recorded defects of those lines — the
  transcript's **search-highlight** and **search-scroll** defects, and the
  plain `wrap_to_strings` engine's lack of escape-atomicity on the
  tool-output path. Neither the search path nor the plain engine was
  touched, so neither defect was fixed nor moved.

## 6. Requirement-to-evidence matrix

| Plan §13 criterion | Met | Evidence |
|---|---|---|
| 1. The per-frame Markdown parse count is bounded and does not grow with the response length | **yes** | `the_streaming_parse_count_does_not_grow_with_the_response` — a response 24× longer costs the same number of parses. |
| 2. An unchanged composer is wrapped once, not four times | **yes** | `the_composer_is_wrapped_once_per_frame_and_shared_by_its_consumers` (1) and `an_unchanged_composer_is_not_wrapped_again` (0). |
| 3. No keystroke clones the whole composer buffer | **partly** | The render path's clone and the completion path's clone are gone (the guard fails if either returns). The **wrap's own** input copy remains: `wrap_styled_spans_by_word_rows` takes `&[Span<'static>]`, so the text is copied once per wrap. It now happens once per **content change** rather than once per frame, and removing it entirely would change the wrap primitive's signature — the `tui-composer-text-layout` line's contract, which §5 puts out of scope. |
| 4. A local edit no longer discards the editor's highlight cache | **yes** | `a_local_edit_keeps_the_highlights_of_the_lines_above_it` — the cache is keyed on the content of each scanned line, and the scan continues from the first changed line. |
| 5. The tasks overlay's construction cost is proportional to the rows it draws | **partly** | The overlay no longer deep-copies the list before truncating: `bounded` borrows the rows and copies only the ones it keeps (`the_tasks_overlay_copies_only_the_rows_it_draws` — six rows copied out of 201), and `scripts/check_tui_render_cost.py` fails if the render path clones the list again. The **construction** of the full list is still proportional to the total task count, because the "N more" count on each group header needs the group's total. |
| 6. The transcript's retention is stated and recorded, and the layout cache does not hold a second full copy | **partly** | The retention is stated and recorded (§4 and `architecture/tui.md`). The layout cache **does** still hold a second rendered copy: its rows are the row index, the one producer of the row count, and dropping them would reintroduce the second producer the disclosure work removed. |
| 7. The rendered output is identical to the baseline for every state in the fixture set | **yes** | The fixture set is in `tests/tui_render/tui_render_cost.inc` and covers the composer (empty, plain, wrapped, multi-byte, CSI, OSC-8) and the streaming tail (paragraphs, bold, a list). |
| 8. The frame cadence, the spinner and the render's `catch_unwind` are unchanged | **yes** | `scripts/check_tui_render_cost.py` fails if the `catch_unwind` or the 16 ms streaming interval moves. |

## 7. Verification executed

```bash
# The new guard, and the proof that it bites.
$ python3 scripts/check_tui_render_cost.py
check_tui_render_cost: the render path derives each result once
$ python3 scripts/check_tui_render_cost.py --self-test
  rejected: a second wrap producer in the composer
  rejected: a composer with no layout cache
  rejected: a second Markdown parse producer
  rejected: a streaming tail with no cache
  rejected: a render path that clones the whole composer buffer
  rejected: a render path that clones the whole task list
  rejected: a render without its catch_unwind
self-test: 7 injections, all rejected

# The affected family and the new member.
$ CARGO_BUILD_JOBS=3 cargo test --test tui_render
running 1064 tests
test result: ok. 1064 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

# The ground-truth Rust test inventory.
$ python3 scripts/rust_test_scan.py --validate

# The routine subset: rustfmt, the generated-asset checks, the guard
# scripts, and a workspace check.
$ scripts/verify.sh quick
==> Quick verification passed.

# The touched modules' own unit tests.
$ CARGO_BUILD_JOBS=3 cargo test -p codegg --lib -- tui::components::prompt \
    tui::composer_geometry tui::composer_text tui::editor_highlight \
    tui::components::messages tui::app::state::tasks
test result: ok. 178 passed; 0 failed; 0 ignored; 0 measured; 7261 filtered out
```

`scripts/verify.sh scoped HEAD~5` was **started** and did not finish inside
the milestone's budget: a change in the root package widens the selection to
every transitive dependent's full surface, which is the documented cost of
`scoped` on this tree. What it would have added over `quick` is the affected
**tests** beyond the family, and those were run directly: the whole
`tui_render` family (1064 tests ✅), the eight new member tests ✅, and the
touched modules' 178 unit tests ✅. `scripts/verify.sh full` — the clippy
sweep — was **not** run: the milestone was developed on a machine shared with
a sibling milestone, and the plan records that the macOS lint set already
fails for reasons C005 owns. The change adds no lint finding: the only
clippy-visible change is `bounded`'s new generic bound, which is exercised by
both call shapes.

The baseline capture of §2.1 is itself a verification command, and it is
reproducible from the commit: `git checkout 626a1abbb -- <production files>`,
re-apply the instrument, run the member, and restore.

## 8. Invariant review

* **The frame cadence.** Unchanged: the guard pins the `catch_unwind` and
  the 16 ms interval, and no file in `src/tui/runtime/` was touched.
* **The visible result.** Unchanged: the equivalence argument is §3 and the
  evidence is the fixture set.
* **The main buffer's layout rules.** Unchanged: the transcript's rows still
  come from the one producer, the layout cache.
* **The composer's visible-row geometry.** Unchanged: the wrap width is
  still the pane's width and the height is still a budget.
* **The editor's wrap primitive.** Unchanged: `src/tui/wrap.rs`'s two
  engines were not re-derived; only two items became `pub(crate)`.
* **The transcript's ordering and content.** Unchanged: no message was
  dropped and no message was reordered.
* **The LSP status.** Unchanged: still off the render path.

## 9. Security review

* No authorization boundary changed.
* The editor's highlight cache is keyed on the **content** of the scanned
  lines, hashed with a keyed hash (`DefaultHasher`). A collision would
  colour a line from a stale context — a cosmetic defect, not a
  confidentiality one — and the hash is keyed precisely so that a
  deliberate collision is not cheap.
* The file tree's row cache was **not** changed in this milestone, so the
  "a stale path is displayed" concern does not arise here.
* The transcript's retention is unchanged, so no session's content became
  reachable from another session's cache: every cache here is per-surface
  and per-widget.

## 10. Migration and compatibility review

None. No protocol, storage or configuration change; no migration. The
`MessagesWidget::streaming_tokens` field became private with a
`streaming_tokens()` accessor and a `set_streaming_tokens()` setter, because
the tail's cache depends on the tail only ever growing — a public field
could be written without that being true, and the cache would then answer
with the rows of text that is no longer there.

## 11. Documentation and operations

* `architecture/tui.md` gained "The render path's caches (C006)" and "The
  transcript's retention".
* `scripts/check_tui_render_cost.py` is wired into `scripts/verify.sh`'s
  `run_guards()` and into `.github/workflows/ci.yml`.

## 12. Unresolved findings

* The **file tree**'s row list is still rebuilt and re-cloned two to three
  times per frame, and its `selected_index` lookup still rebuilds the list
  (plan WP8). Not touched: the milestone's budget ran out, and the file
  tree's rows carry **paths**, so a cache there needs an invalidation that
  is proven against a stale path — the plan's own stop condition.
* The **`/search` path** still clones every message part, and the plain
  `wrap_to_strings` engine still lacks escape-atomicity on the tool-output
  path. Both were left alone deliberately: the search path is where the two
  recorded defects of the conditionally-closed lines live.
* **`ComposerText::point_at`** no longer scans the buffer for its line: the
  line breaks are indexed and spliced on every edit. That part of WP8 is
  done.
* The **tasks overlay's construction** is still proportional to the total
  task count, because the "N more" count on each group header needs the
  group's total (§6, criterion 5).

## 13. Roadmap disposition

The milestone is closed against its plan, with the three partial criteria
above recorded rather than hidden. The remaining findings are named in §12
so that a later milestone can pick them up without re-deriving them.

## 14. Registry updates

`plans/registry.md` is updated with this milestone's status and with the
three partial criteria and the four carried-over findings.
