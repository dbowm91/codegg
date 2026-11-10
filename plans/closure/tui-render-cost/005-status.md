# TUI Render Cost Milestone 005 — Closure Status

Status: **closed** (2026-11-10), with four recorded deferrals and three
deliberate deviations from the plan's wording (§8).

Implementation plan:
`plans/implementation/tui-render-cost/005-editor-draw-path-cost.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M005)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created. **ADR-0011 is not reopened.**

## 1. Executive finding

A visible editor row was read **whole** and walked **past** the column window.
`line_spans` read the line with `read_bytes` — a `String` copy of the entire line,
up to the per-line render budget — and its character loop had no early exit: once
`display >= limit` the drawn window was settled, but the walk decoded every
remaining character anyway. So a row's cost was `O(the line's bytes)` rather than
`O(the drawn columns)`. The same visible window was then read a **second** time by
`resolve_viewport`'s `longest_visible_width`, immediately before `line_spans` read
it again.

Three further costs sat on top: the semantic-token scan was linear over **all**
tokens for every row; the per-segment `syntax_ranges` lookup restarted from index 0
for every segment; and the highlight cache's `first_changed_line` re-read,
re-hashed and re-highlighted the whole scanned prefix on every keystroke — so
editing line 4,900 of a 5,000-line file touched ~4,900 lines.

## 2. The cost sites, before and after

Per **warm** frame (the second draw of the same state) at a 40-row text area in a
126×43 frame.

| Fixture | `editor_row_reads` | `editor_row_bytes` | `editor_token_scan_steps` |
|---|---|---|---|
| 40 rows over short lines (60 × ~18 B) | **80 → 40** | 1,400 → 700 | 0 → 0 |
| 40 rows over long lines (60 × 4,000 B) | **80 → 40** | **320,000 → 19,520** | 0 → 0 |
| 40-row viewport over one 20,000 B minified line | **4 → 2** | **40,000 → 492** | 0 → 0 |
| 1,000 semantic tokens over 1,000 lines | 80 → 40 | 1,400 → 700 | **40,000 → 480** |

`highlight_prefix_lines`, 5,000-line document:

| Edit position | Before | After |
|---|---|---|
| line 10 | 11 | **1** |
| line 2,500 | 2,501 | **1** |
| line 4,900 (below the scan) | 4,900 | **0** |
| line 10, no seed available | 11 | 11 (the correctness fallback) |

### 2.1 The captured baseline failures

```
a warm frame reads one window per row, not two: 80 reads for 40 rows
  left: 80  right: 40
a minified line ...:  left: 4  right: 2
a row's scan must visit the row's tokens, not the document's:
  40000 steps for 40 rows over 1,000 tokens, bound 480
an edit on line 10 of 5,000 must verify from the edit, not from line 0:
  11 lines checked, bound 1
```

The baseline **byte** figures and the 2,500 / 4,900 prefix figures come from a
faithful reconstruction of the pre-change algorithm (whole-row `read_bytes` plus
an always-missing clamp; `let from = 0`). That reconstruction reproduced the true
baseline **read counts** exactly (80 / 80 / 4 / 80), which is the cross-check that
it measured the right thing. It was reverted and the tree re-verified.

## 3. The shape

- `row_window` / `visible_window` / `boundary_at_or_before` / `head_reach` compute
  the drawn window from a bounded prefix and exit the walk at the window's end.
  `head_reach` is the one place the read extends beyond the window, because view
  mode can place a head cell past it.
- The clamp memo is `ClampCache` keyed on
  `(snapshot.revision(), first_line, visible_lines, text_columns)`, holding **one
  `usize`** — the widest visible line in columns — and nothing else. It lives on
  `TuiDocumentPresentation` and is explicitly cleared by `reset_for_open`.
- The token scan is `partition_point` over the sorted token vector, then the row's
  range only.
- The style lookup is **two** monotone cursors, not one (see §8).
- `first_changed_line(snapshot, from)` is seeded from the edit by
  `note_edit_from`, taken on the post-edit snapshot in the two mutation funnels
  (`submit_edit`, `traverse_history`) and `min`-combined; unseeded, it falls back
  to the 0-based scan.
- `ScanKey` compares its `&str` fields before allocating; `byte_at` on the snapshot
  adapter is one rope lookup instead of a `String` per byte.

## 4. The prefix matrix — the correctness direction

| Direction | Test | Result | Prefix lines after |
|---|---|---|---|
| edit **above** the cached scan (edit 3, scanned to 21) | `an_edit_above_the_cached_scan_is_still_detected` | pass — spans equal a fresh scan, edited row recoloured | 4 (no seed) |
| edit **below** the scan (edit 30, scanned to 21) | `an_edit_below_the_scan_reads_no_prefix` | pass — no scanned row recoloured | **0** |
| edit **near the end** (38 of 40; 4,900 of 5,000) | `an_edit_near_the_end_starts_the_check_at_the_edit` | pass | **1** |
| no seed available | `an_unplaced_edit_still_scans_from_the_top` ×2 | pass — walks from 0, still detected | 4 / 11 |
| two edits between frames | `the_seed_keeps_the_earliest_edit` | pass — the seed is the `min` | — |
| the keyed hash over rope chunks | `the_keyed_hash_is_a_stream_and_not_a_slice` | pass | — |

## 5. The perturbations — all recorded and reverted

1. **Clamp cache, drop `revision` from the key.** Makes
   `a_frame_after_an_edit_draws_what_a_cold_frame_draws` fail: `the clamp kept the
   pre-edit width and could not scroll to the new edge`. The differing cell is the
   cursor's — cold `x: 44, y: 1 … modifier: UNDERLINED`, warm `x: 44 …
   modifier: NONE`. Reverted → passes.
2. **A token whose end is past the window** (`None if false => line.len()`) makes
   `a_token_running_past_the_window_colours_what_is_on_screen` fail — the token is
   dropped and the row loses its colour. This was a **real defect introduced and
   caught in review**; fixed by clipping the token to the window. Reverted → passes.
3. **A read budget landing inside a character** (`let read_end = budget`) made
   `a_multi_byte_row_draws_its_whole_window` fail with `left: ""` — the whole row
   blank, because `read_bytes` refuses a non-boundary range. Also a **real defect
   caught in review**; fixed with `boundary_at_or_before` and covered permanently.

The third finding is why the integration member carries
`every_fixture_draws_its_document`: a warm/cold comparison cannot see a row that
both sides draw empty.

## 6. Invariant review

- **ADR-0011 holds.** No second buffer, no `Rope`, no materialized `Vec<Line>`, no
  cached document copy; the clamp memo holds an integer. The editor's five guards
  are green with **no allowlist change** (§9).
- **The rendered output is byte-identical** — horizontal clip, gutter,
  current-line tint, selection bands, truncation suffix, semantic-token override —
  over the fixture set: a short file, a long line clipped horizontally, a
  multi-byte character at the clip, a selection, a cursor on the last line, tokens
  overriding a scan style, the truncation suffix, and a horizontally scrolled view.
- **The bounded-rendering invariant (C002) is unchanged and tighter**, not looser.
- **The highlight cache's content keying is unchanged**;
  `a_local_edit_keeps_the_highlights_of_the_lines_above_it` still passes.
- **The keyed hash stays keyed** (`DefaultHasher`), so a deliberate collision is
  not cheap.
- **One producer of a row's spans.** The cached spans remain the only source.

## 7. Security review

No authorization boundary. Two checks the plan named were honoured: the highlight
cache is keyed on line **content** with a keyed hash, and reducing the prefix it
re-reads did not reduce what it verifies — an edit **above** the scan is still
detected, proved by its own test and by the perturbation in §5. No path, document
content or buffer byte is retained beyond the widget's existing lifetime; the
clamp cache holds an integer.

## 8. Deviations and deferrals

- **The style lookup is two monotone cursors, not one.** `syntax_ranges` is two
  ascending runs — the server's tokens, then the built-in scan — and a single
  cursor over the concatenation is **wrong**: for `scan = 0..10`, `lsp = 5..6`, the
  segment `0..5` must take the scan's colour, and a pointer already sitting on
  `5..6` would fall to `base`. A byte-sorted merge is also wrong (it gives `5..6`
  the scan's colour). Two cursors preserve the run order, which **is** the
  priority. Proven by `the_style_sweep_keeps_the_scan_beside_a_partial_token`,
  which fails under both single-cursor shapes.
- **The row's read is a prefix from the line's start, not from the scroll
  column.** Every offset the row builds (token columns, search matches, cell
  ranges) is line-relative, so a window not beginning at the line's start could
  not be resolved. `editor_row_bytes` is therefore bounded by the **window's end
  column**; for a deeply scrolled view it approaches the line and never exceeds
  the previous cost.
- **`line_slice` — the >32 KiB fallback row — is untouched.** §5 names only
  `line_spans`' read; `line_slice` already breaks its walk at the column window and
  its clip rule deliberately differs.
- **"Allocates nothing on a hit" and "a word motion does not allocate per byte" are
  proved structurally, not by an allocation counter.** The key comparison is a
  `&str` comparison before any `to_string`, pinned by the `editor-scan-key-alloc`
  guard; `byte_at` is one rope lookup, pinned by
  `a_string_and_a_snapshot_agree_on_every_byte`. A counting global allocator would
  be process-wide state in a test binary that runs cases in parallel — the
  measurement theatre R5 warns about.
- **`crates/codegg-document` was touched, additively**: `byte_chunks(start, end)`
  (borrowed rope leaves, same range validation as `read_bytes`) and
  `byte_at(offset)`. `read_bytes`'s signature is unchanged, no buffer is retained,
  and the crate's own tests cover both.

## 9. Verification executed

```text
$ cargo fmt --check
exit=0
$ cargo test -p codegg --lib -- tui::components::editor tui::editor \
    tui::editor_highlight tui::text_geometry
test result: ok. 349 passed; 0 failed; 0 ignored; 0 measured; 7543 filtered out
$ cargo test -p codegg-document
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ cargo test --test tui_render -- tui_editor_cost
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 1170 filtered out
$ cargo test --test tui_render
test result: ok. 1183 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 scripts/check_tui_editor_text_authority.py
TUI editor text-authority guard passed — no retained text buffer and no direct
filesystem reads in the editor path
$ python3 scripts/check_tui_editor_key_authority.py
TUI editor key-authority guard passed — 2 guard-block chord(s), 74 Normal-mode
chord(s), 8 Insert-mode chord(s), no shadowed arm
$ python3 scripts/check_tui_editor_prefix_state.py
TUI editor prefix-state guard passed — one pending-prefix field, typed as
EditorPrefix, no legacy mechanism left
$ python3 scripts/check_tui_editor_leaf_authority.py
TUI editor leaf-authority guard passed — 6 leaf table(s), 52 leaf glyph(s),
13 prefix binding(s), no duplicate and no unreachable leaf
$ python3 scripts/check_tui_editor_doc_drift.py
check_tui_editor_doc_drift: 52 leaf gestures and 83 chords are documented
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 30 injections, all rejected
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

**No allowlist entry was added anywhere.** The only guard that changed is
`check_tui_render_cost.py`, and it got stricter: this milestone added **six**
checks and **nine** injections, taking the file to 30 injections in total.

The family count is the count at this milestone's closure. The tree-level
`scripts/verify.sh quick` result is recorded once, in the roadmap's closure.

## 10. Findings handed to the roadmap closure, and registry update

Two blockers were handed back and are **fixed in the roadmap's closure**, not
here, because both are cross-milestone:

- `scripts/check_comment_budget.py` was red with 54 R4 violations, 39 of them from
  M001–M004. It was banked at the roadmap's closure with the reason recorded.
- A `field_reassign_with_default` warning in M003's test would have failed
  `clippy -D warnings`. Fixed at the roadmap's closure.

`architecture/overview.md`'s integration-test count was already stale for four of
the five cost members before this milestone; it now names six.

The roadmap's milestone table records M005 as `closed` with this record linked;
`plans/registry.md` moves its row to the closed disposition. The unblock audit
found nothing to unblock.
