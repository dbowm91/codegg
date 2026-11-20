# TUI Render Cost Milestone 004 — Closure Status

Status: **closed** (2026-11-10), with one recorded deferral (§8) and four
pre-existing findings recorded rather than fixed.

Implementation plan:
`plans/implementation/tui-render-cost/004-transcript-rebuild-cost.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M004)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created.

## 1. Executive finding

C006 built the transcript's caches; this milestone removed the copies and the dead
work **around** them. The streaming tail's fast path was commented "the rows are
already rendered" and then rendered them again — every block's every row
deep-cloned into a fresh `Vec<Line>` on every call, for a tail that is the whole
response received so far. A cached message part's rows were cloned twice per
rebuild (once out of the cache, once by the caller), three times under a search
match. A collapsed tool call parsed its JSON payload and built a formatted outline
that was then dropped, because the build ran **before** the `expanded` gate — and
collapsed is the disclosure default. And the code-block wrapper emitted one `Span`
plus one heap `String` **per character**, so an 80×100 block was ~8,000
allocations and ~8,000 spans for ratatui to resolve individually.

## 2. The cost sites, before and after

| Counter | Fixture | Before | After |
|---|---|---|---|
| `tail_row_copies` | frame that rebuilt the transcript, text unchanged | 1 | **0** |
| `tail_row_copies` | one appended token | 1 | 1 |
| `part_row_copies` | rebuild of a 5-part message | **6** | **3** |
| `part_row_copies` | same, live search matching a prose part | **7** | **3** |
| `tool_body_builds` | collapsed `websearch` call | **1** | **0** |
| `tool_body_builds` | same call expanded | 1 | 1 |
| `code_block_spans` | 1 row × 80 chars, width 80 | **80** | **1** |
| `code_block_spans` | 20 chars, 2 runs, width 8 (3 rows) | **20** | **4** |
| `code_block_spans` | 20 rows × 40 chars | **800** | **20** |
| `code_block_spans` | 100 rows × 80 chars | **8,000** | **100** |

### 2.1 The captured baseline failures

```
a collapsed call draws no body, so it parses no payload: 1 parses   left: 1 right: 0
an unchanged tail must serve its cached assembly: 1 full re-copies   left: 1 right: 0
one copy per drawn part, not two: 6 copies for 3 parts   left: 6 right: 3
the highlighted part is appended from the borrow: 7 copies for three parts   left: 7 right: 3
one span emitted for one run, not one per character: 80 spans   left: 80 right: 1
one span per run segment per row: 20 spans   left: 20 right: 4
one span per row for one style run: 800 spans for 20x40   left: 800 right: 20
one span per row for one style run: 8000 spans for 100x80   left: 8000 right: 100
```

## 3. The shape

- `StreamingTailCache` holds its **assembled** rows plus an `assembled_ends`
  index and returns them by borrow: `render(&mut self, …) -> &[Line<'static>]`,
  appended by the caller with `extend_from_slice`. `Rc` was not needed and is not
  present. On a block change the assembly is a fold that re-folds only
  `blocks[from..]`, so a streaming tick does not pay two full deep copies. The
  assembly **rule** is byte-for-byte the old loop, split into `extend_assembly`
  with `assemble_markdown_blocks` a thin wrapper over it.
- `markdown_part_rows` hands back a borrow; both call sites extend from `&[Line]`.
  `highlight_match_in_rendered` became `highlighted_first_row` plus an in-place
  append, so a search match no longer adds a third copy.
- The tool-call body build moved **inside** the `*expanded` gate. The collapsed
  header's number is preserved by a count-only structural read.
- `wrap_code_spans` accumulates a contiguous run and emits one `Span` per style
  run per row.
- `render_numbered_code_output` builds one owned buffer and takes `&str` slices;
  the caller's pre-join is gone. Peak materializations of the payload text:
  **3 → 1** by inspection (no honest counter exists for that one).

## 4. The equivalence argument and its perturbation

C006's `the_streaming_tail_renders_the_same_rows_as_a_fresh_parse` passes
unchanged, and `a_message_parts_markdown_is_parsed_once` passes unchanged. The
milestone added a stronger comparison: the streamed frame is compared against a
fresh parse of the same prefix at **every three-character prefix** over seven
fixtures (a paragraph, a heading plus paragraph, a list, two fenced code blocks,
CJK prose, and an OSC-8 hyperlink).

**The perturbation, recorded and reverted.** Replacing the block-change path with
`return &self.assembled;` (serve stale rows while the blocks moved) makes the
comparison fail at the first six-character prefix:

```
the incrementally parsed tail must draw what a fresh parse of the same 6-byte
prefix draws for "a single paragraph of prose":
Generating...
a s
```

and `a_streaming_tick_reassembles_the_tail_once` reports `0 assemblies`
(left 0, right 1). Reverted; the tree re-verified.

**The perturbation initially did not bite**, because the first fixture never drew
mid-stream and so never took the incremental path. The fixture was strengthened to
draw after each chunk. A comparison that cannot fail is not evidence, and this is
recorded because the first version was exactly that.

## 5. Invariant review

- **The rendered output is byte-identical**, including the blank line between two
  Markdown blocks and its absence where the previous block already ended with one.
- **One producer of the transcript's rows.** `build_message_rows` still emits them;
  the layout cache is still their index; the scrollbar's row count and the drawn
  rows are the same rows. This milestone removed copies, not producers.
- **The disclosure model is unchanged.** A collapsed call shows the same header,
  the same hidden-output counter and the same status row. Only the body that is
  not drawn stops being built.
- **The wrap's byte offsets and styles survive.** `wrap_code_spans` changed how
  many spans it emits, not where a row breaks or what style a character carries.
- **`highlight_code` is not re-derived.**
- **The hidden-output number is identical.** Proved by
  `the_collapsed_header_counts_the_rows_the_expanded_body_draws` for a framed
  web-search JSON payload (header `(+8 lines)`, expanding adds exactly 8 — the
  outline's row count; pretty-printing would be 13 and the frame's own text 6, so
  substituting either cheaper number fails the test) and for a plain 4-line
  `bash` output.

## 6. Security review

No authorization boundary. The one check the plan named was honoured:
`render_structured_output_outline` reads payloads that may be externally framed,
and `extract_trust_frame_payload_for_display` still runs first and still receives
the same input. The trust frame is still never rendered as if it were the result.
The payload is display-only either way; the model path is untouched.

## 7. Migration and compatibility review

None outside `src/tui/components/messages.rs`. No protocol, storage,
configuration or migration change. No public API changed.

## 8. Unresolved findings and deferrals

- **`render_md_line_wrapped` is deferred, and the reason is stronger than the plan
  anticipated.** Routing its overflow path through `wrap_styled_spans_by_word`
  would **keep** bold/italic/code styling on continuation rows that the current
  path deliberately drops. That is a rendered-output change, and byte-identical
  output is this milestone's hard constraint, so it needs its own milestone. The
  plan permits leaving it as the lowest-value item.
- **The counter is named `tool_body_builds`, not the plan's
  `collapsed_payload_parses`, and the rename is deliberate.** The plan's
  acceptance evidence asks for 0 on a collapsed call. That is unachievable without
  changing the displayed number: for a *structured* payload the exact `(+N lines)`
  count **is** the parsed outline's row count, so the collapsed header still
  performs one count-only structural read. A counter named "payload parses" that
  reads 0 while a parse happens is measurement theatre; the counter now counts
  bodies, and the collapsed path is recorded as **body-free, not parse-free** in
  the counter's doc, the test member and `architecture/tui.md`.
- **The streamed-table defect is fixed** in the corrective pass of 2026-11-10.
  A chunk boundary that left a bare `---` as the last block made it a block of
  its own, so the re-parse started after the table's header row and the table
  could never form. A block's meaning can depend on the block **before** it, so
  `reparse_start_line` now backs up one block — one extra block re-parse per
  tick, bounded by a block rather than by the response. The defect test became
  `a_table_forms_when_its_delimiter_arrives_after_a_bare_break`, and the table
  joined `INCREMENTAL_FIXTURES` so the every-prefix comparison covers it.
- **The suppressed reasoning header is fixed** in the same pass: the search arm
  skipped the `prev_was_reasoning` reset, so a searched prose part dropped the
  header of the reasoning block after it. The reset is now unconditional, and
  `a_searched_prose_part_suppresses_the_next_reasoning_header_known_defect`
  became `a_searched_prose_part_still_draws_the_next_reasoning_header`.
- **The part cache's key is the part's length**, which is sufficient today because
  every write path appends. A same-length rewrite would serve stale rows;
  recorded in `a_same_length_replacement_of_a_part_serves_the_cached_rows_known_limitation`.
  Widening the key would put a hash of the whole response on the per-token path.
- **`a_message_parts_markdown_is_parsed_once` now really has five parts.** Its
  comment claimed five while the tag form yielded one, because `find_any_tag`
  requires a non-alphanumeric character after `<thinking>`. The fixture builds
  its parts explicitly in the corrective pass, so the test measures what it
  claims; the assertion still holds.

## 9. Verification executed

```text
$ cargo fmt --check
exit=0
$ cargo test -p codegg --lib -- tui::components::messages
test result: ok. 91 passed; 0 failed; 0 ignored; 0 measured; 7782 filtered out
$ cargo test --test tui_render -- tui_transcript_cost
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1159 filtered out
$ cargo test --test tui_render
test result: ok. 1170 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 scripts/check_tui_render_cost.py
check_tui_render_cost: the render path derives each result once
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 21 injections, all rejected
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

The family count is the count at this milestone's closure. The tree-level
`scripts/verify.sh quick` result is recorded once, in the roadmap's closure.

## 10. Roadmap disposition and registry update

M004 is **closed**. It does not touch the plain `wrap_to_strings` engine's lack of
escape-atomicity on the tool-output path — that is `plans/closure/repository-audit-corrective/006-status.md`
§12's third carried finding and remains **not owned** by this roadmap, as M006
also records.

The roadmap's milestone table records M004 as `closed` with this record linked;
`plans/registry.md` moves its row to the closed disposition. The unblock audit
found nothing to unblock.
