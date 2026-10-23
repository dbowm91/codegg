# TUI Main Buffer Text Rendering Milestone M001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-main-buffer-text-rendering/001-word-aware-wrapping-in-the-main-buffer.md`

Source subsystem roadmap:

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m001--word-aware-wrapping-in-the-main-buffer`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are `src/tui/wrap.rs` (the
  styled primitive), `src/tui/components/messages.rs` (the markdown path that
  switches to it), `tests/tui_render/tui_main_buffer_row_accounting.inc`,
  `tests/tui_render/tui_main_buffer_word_wrap.inc`,
  `architecture/tui.md` and `.opencode/skills/tui/SKILL.md`. No SHA, pull
  request number or commit message is claimed, because none exists: the
  implementing agents were not authorized to commit.

## 1. Executive finding

The milestone's capability boundary — **does model-authored text in the main
buffer break at word boundaries, with its styles and byte offsets intact** — is
complete in the working tree, and the completion is verified by measurement
rather than by assertion.

At the baseline the main buffer ran the model's prose through a
**character-aware** wrapper that emitted **one `Span` per character**. A
paragraph written in whole words was drawn as a stream of single-character
spans: the row boundaries fell wherever the character count said they should,
so a word could be split across two rows, and the per-layout allocation grew
linearly with the length of the answer. The milestone replaces that path with a
**styled, word-aware, escape-aware** primitive that breaks at whitespace,
preserves each word's style, reports the source's byte range per row and per
slice, and treats a terminal control sequence as one atomic, zero-width token.

What landed:

- `src/tui/wrap.rs` — `StyledSlice { text, style, source_start, source_end }`,
  `WrappedRow { slices, source_start, source_end, width }`,
  `wrap_styled_spans_by_word_rows`, `wrap_styled_spans_by_word` and
  `visible_width`.
- `src/tui/components/messages.rs` — the markdown rendering path
  (`wrap_markdown_spans` → `wrap_styled_spans_by_word`) now uses it, so every
  markdown text node word-wraps. **Fenced code blocks and the `read` tool's
  output stay character-aware**, by decision: a code block broken mid-word is
  normal, and code must not be re-flowed at word boundaries.
- The plain string engine (`wrap_to_strings`, `wrap_count`,
  `ellipsize_to_width`, `visible_width`) is unchanged in shape.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate. The exact future evidence that discharges the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate.

**The risk.** A local `scripts/verify.sh quick` run is a property of **this
machine and this tree**, not of the hosted runner. The hosted lane is a strict
**subset** of `quick` — it omits the client and desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards —
so a green hosted run does **not** by itself prove everything the local gate
proves. The local gate's own value is what this record states in §4: it is
strong evidence about this tree, and it is **not** evidence about the hosted
runner's environment, its runner image, or its concurrency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — the main buffer's assistant prose, reasoning bodies and streaming tails break at word boundaries; a row ends mid-word only when the word alone is wider than the row | `wrap_styled_spans_by_word_breaks_at_word_boundaries`, `wrap_styled_spans_by_word_hard_breaks_an_over_long_token` (`src/tui/wrap.rs`); `wrap_markdown_spans` → `wrap_styled_spans_by_word` (`src/tui/components/messages.rs:3205`, `:3226`) | pass | The hard-break rule is the plain engine's rule, so the two engines agree by construction. |
| §13.1 — the three model-authored paths reach the primitive | `NodeValue::Paragraph` (`src/tui/components/messages.rs:2818`), `NodeValue::Heading` (`:2822`) and the residual `_` arm (`:2893`) all call `wrap_markdown_spans`; block quotes, list items, description lists, alerts and footnotes reach it by recursing into `render_markdown_blocks` (`:2841`, `:2890`, `:2922`) | pass | The switch covers **every markdown text node**, not only the three the plan named. |
| §13.2 — a styled span's style is preserved across a word boundary | `wrap_styled_spans_by_word_preserves_a_style_across_a_word_boundary` | pass | A span is split at word boundaries rather than flattened. |
| §13.2 — a selection-range-like byte interval maps to the wrapped row that contains it | `wrap_styled_spans_by_word_reports_each_row_source_byte_range`; `WrappedRow { source_start, source_end }` and `StyledSlice { source_start, source_end }` | pass | `source_start` is inclusive, `source_end` exclusive, both offsets into the concatenation of the input spans' contents. |
| §13.3 — an OSC-8 hyperlink span survives a wrap whole | `wrap_styled_spans_by_word_keeps_an_osc8_span_whole`; `assistant_prose_keeps_an_osc8_auto_link_whole_across_a_wrap` (`src/tui/components/messages.rs`) | pass | The consumer-level test is the one that proves the whole path, not just the primitive. |
| §13.3 — its printable parts do not consume the row width | `wrap_styled_spans_by_word_does_not_count_escape_printable_parts_toward_the_width`, `visible_width_ignores_escape_sequences` | pass | A hyperlink's target is a parameter of the sequence, not text; measuring raw bytes counted it at roughly `2n + 8` columns. |
| §13.4 — the scrollbar's row count equals the number of rows drawn, for a message with a tool call, a reasoning block and prose | `the_rows_the_transcript_draws_are_the_rows_the_row_count_reports` and `the_scroll_range_is_the_row_count_minus_the_viewport` (`tests/tui_render/tui_main_buffer_row_accounting.inc`); `the_transcript_draws_the_rows_the_scrollbar_counts` and `a_message_with_a_tool_call_a_reasoning_block_and_prose_renders_the_sum_of_its_parts_rows` (`tests/tui_render/tui_main_buffer_word_wrap.inc`) | pass | One producer for rows and for the row count — I1. |
| §13.5 — the two search functions have tests, and the known defects are recorded rather than absorbed | `highlight_match_in_rendered_returns_lines_unchanged_for_non_first_logical_line_known_defect`, `highlight_match_in_rendered_compares_byte_offsets_against_display_columns_known_defect`, `scroll_to_current_search_match_adds_visual_row_offset_to_logical_line_count_known_defect` (`src/tui/components/messages.rs:5741`, `:5767`, `:5789`) | pass | The defects are **not fixed**; the tests name them. See §10. |
| §15 — the primitive's unit tests, with the escape-sequence integrity case | 17 `#[test]` functions in `src/tui/wrap.rs` | pass | The escape-sequence cases are `wrap_styled_spans_by_word_keeps_an_osc8_span_whole`, `…does_not_count_escape_printable_parts_toward_the_width`, `…does_not_split_a_span_that_is_only_an_escape_sequence`. |
| §15 — the model-authored paths' rendered output, showing word boundaries | `assert_breaks_at_word_boundaries` helper (`tests/tui_render/tui_main_buffer_word_wrap.inc:108`) plus the two rendering tests in that member | pass | The integration member asserts the property, not a snapshot. |
| §15 — an integration assertion that the scrollbar's `total_lines` equals the drawn row count (I1) | `the_rows_the_transcript_draws_are_the_rows_the_row_count_reports` | pass | — |
| §15 — the two search functions' baseline and post-change behaviour, both recorded, with any difference attributed | §4's "the search functions under the new wrap" subsection below | pass | The difference is attributed to the wrap's slack at each row end, not to a behaviour change in either function. |
| §15 — a statement that the model-facing path was not touched (I3) | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; the diff touches `src/tui/` and `tests/tui_render/` only | pass | The model-facing path is `src/search_backend/framing.rs`, and it has no diff lines. |
| §15 — the measured effect on per-layout span allocation, reported as a measurement | §4's measurement table below: 290 spans → 82 slices and 409 spans → 118 slices at width 60 | pass | Measured, not estimated. The **row count rose by one per paragraph** and that is stated too. |
| §15 — the disposition of every existing transcript test that was rewritten or kept | §3's "Existing tests" subsection below | pass | No existing transcript test was deleted; the rewrites are enumerated. |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's verification table | **partial** | The gate is **green on the working tree** (exit 0, "Quick verification passed", 74 steps). What is missing is the other half of the item: **there is no closing commit**, so the gate has not been green *at a closing commit*, and the hosted `CI / verify` lane has not run against this code. This is the milestone's condition; see §1 and §11. |
| The change is measurable rather than asserted | `wrap_styled_spans_by_word_emits_word_sized_slices_not_one_per_character` | pass | The primitive emits word-sized slices, not one per character. |
| The two engines agree | `wrap_styled_spans_by_word_agrees_with_the_plain_word_aware_engine` | pass | The over-long-token rule is shared by construction. |
| Degenerate inputs do not panic or produce zero rows | `wrap_styled_spans_by_word_on_an_empty_source_returns_one_empty_row`, `wrap_styled_spans_by_word_with_a_zero_width_returns_the_source_unwrapped`, `zero_width_is_passed_through_unwrapped` | pass | The row count is never zero for a non-empty buffer. |

## 3. Production implementation evidence

**The primitive (`src/tui/wrap.rs`).**

- `pub struct StyledSlice { text: String, style: Style, source_start: usize, source_end: usize }`
  — the smallest unit the wrap emits: a contiguous run of the source that
  shares one style, plus the byte range it came from. `StyledSlice::display_width`
  measures the slice through `visible_width`, so escape sequences are excluded.
- `pub struct WrappedRow { slices: Vec<StyledSlice>, source_start: usize, source_end: usize, width: usize }`
  — one visual row, with the source byte range behind it and its display width.
  `WrappedRow::text()` returns the row's text with escape sequences verbatim.
- `pub fn wrap_styled_spans_by_word_rows(spans: &[Span<'static>], width: u16) -> Vec<WrappedRow>`
  — the primitive.
- `pub fn wrap_styled_spans_by_word(spans: &[Span<'static>], width: u16) -> Vec<Line<'static>>`
  — the primitive's `Line` projection, which is what the renderer consumes.
- `pub fn visible_width(s: &str) -> usize` — the display width of a string in
  terminal columns, ignoring escape sequences.

The contract: words are placed **greedily**, separated by a **single space**,
with **trailing whitespace dropped**; a token wider than the row is hard-broken
at character boundaries; each word keeps its style; each row and each slice
reports the source's byte range; and a terminal control sequence is one
**atomic, zero-width** token that never ends a word, never consumes width and
is never split across rows.

**The call site (`src/tui/components/messages.rs`).**

- `wrap_markdown_spans` (`:3205`) is now the single entry point for the
  markdown text path, and it calls `wrap_styled_spans_by_word` (`:3226`).
- The **fallback style is applied at the call site**: a span whose style is
  `Style::default()` is re-styled with the paragraph's colour before wrapping
  (`:3216`–`:3225`), because the primitive has no fallback-style parameter and a
  default-styled span would otherwise be wrapped unstyled.
- The paths that reach it: `NodeValue::Paragraph` (`:2818`),
  `NodeValue::Heading` (`:2822`) and the residual `_` arm (`:2893`).
  Block quotes (`:2841`), list items (`:2922`), description lists, alerts and
  footnotes (`:2890`) reach it by recursing into `render_markdown_blocks`, so
  their text nodes land on the same wrapper.
- **What deliberately does not change:** `NodeValue::CodeBlock` still goes
  through `render_code_block` (`:2837`), and the `read` tool's numbered output
  stays on `wrap_code_spans` (`:3229`). Both are character-aware.

**The plain engine.** `wrap_to_strings`, `wrap_count`, `ellipsize_to_width` and
`visible_width` are unchanged in shape.

**Tests.**

- `src/tui/wrap.rs` — 17 unit tests (word boundaries, over-long token
  hard-break, style across a word boundary, byte range per row, empty source,
  zero width, OSC-8 kept whole, escape printable parts not counted toward the
  width, a span that is only an escape sequence, agreement with the plain
  engine, word-sized slices rather than one per character, and the ellipsis
  budget test added by the parent — see §4).
- `src/tui/components/messages.rs` — the consumer-level OSC-8 test
  `assistant_prose_keeps_an_osc8_auto_link_whole_across_a_wrap`, plus the
  three search-coverage tests.
- `tests/tui_render/tui_main_buffer_row_accounting.inc` — 3 integration tests.
- `tests/tui_render/tui_main_buffer_word_wrap.inc` — 2 integration tests.

**Existing tests rewritten.** No existing transcript test was deleted. The
rewrites are confined to the new members' own helpers and to the search
functions' three tests, which are new rather than rewritten: the baseline had
**no coverage** for `highlight_match_in_rendered` or
`scroll_to_current_search_match`, which is precisely what §13.5 and the plan's
work package A asked for. The tests that assert the **known defects** are
deliberate exceptions to the "a new test must fail against the baseline" rule:
they pass against the baseline *because* the defect is present, and their names
end in `_known_defect` so a reader knows they are documenting, not asserting a
correct behaviour.

**Documentation.**

- `architecture/tui.md` — "The styled, word-aware wrap": the two engines, the
  primitive's contract, which paths use it, the call-site fallback rule, the
  measured effect, and the plain engine's continued existence.
- `.opencode/skills/tui/SKILL.md` — the module guide's statement that the
  model-authored text wraps through the shared styled primitive, and that code
  blocks and the `read` tool's output stay character-aware.
- `architecture/overview.md` — the integration-test-file count row, which this
  milestone's two new members moved.

## 4. Verification executed

### Commands run

```bash
# recorded at this milestone's own verification point
cargo test --locked -p codegg --lib tui::wrap
cargo test --locked -p codegg --lib tui::components::messages
cargo test --locked -p codegg --test tui_render
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo check --workspace --all-targets --locked
cargo fmt --check --all
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::wrap` | **17 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::components::messages` | **48 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1036 passed**, 0 failed |
| `python3 scripts/check_test_assertion_quality.py` | all checks pass |
| `python3 scripts/check_test_duplication.py` | all checks pass |
| `cargo check --workspace --all-targets --locked` | exit 0 |
| `cargo fmt --check --all` | clean |
| `bash scripts/verify.sh quick` | **exit 1 — one step** (at this milestone's verification point — see the re-verification below for the current result) |

**`verify.sh quick` failed at exactly one step at this milestone's
verification point:**
`check_test_target_references` reported that `architecture/overview.md`
recorded a stale integration-test-file count. That row is a **documentation
count**, not a product behaviour: the guard compares the row against the tree,
and the row had not been re-measured after this line's new integration members
were added. Every other step of the gate passed; the remainder was run
individually and passed. **The row has since been re-measured and corrected in
the same working tree, and the gate is now green** — see the re-verification
below and §9.

### Re-verification (this record's author, on the same working tree)

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::wrap` | **17 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --lib tui::components::messages` | **80 passed**, 0 failed — **higher** than the 48 recorded above, because M002 and M003 of this same roadmap added unit tests to the same module after this milestone's verification point |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed — **higher** than the 1036 recorded above, for the same reason |
| `cargo test --locked -p codegg --lib` | **7373 passed, 3 failed** — the three environmental failures in the table below, and nothing else. |
| `bash scripts/verify.sh quick` | **exit 0 — green**: "Quick verification passed", 74 steps. The one red step seen earlier in this record's writing — `check_test_target_references`, "architecture/overview.md records 326 integration test files, but the tree holds 327 root integration test modules" — was a **stale documentation count**, re-measured and corrected in the same working tree. |

The recorded test counts are **snapshots of a tree that other agents were
concurrently modifying** — the main-buffer and composer milestones of this line
were implemented in parallel, and each milestone's integration members landed in
the same family. They are therefore **not a monotonic series** across the five
records of this line, and each record's count describes the tree at that
milestone's own verification point.

### The measurement

Measured, not estimated. At **width 60**, comparing the baseline's
character-aware engine with the shipped word-aware engine on the same span
vector:

| Paragraph | Before | After |
|---|---|---|
| 291 chars | 290 spans, 5 rows | **82 slices, 6 rows** |
| 411 chars | 409 spans, 7 rows | **118 slices, 8 rows** |

A **3.5× reduction** in per-layout slice allocation. The **row count rose by
one per paragraph**: the word-aware wrap leaves slack at each row end where the
character-aware wrap filled every row to the exact column. The transcript's
scroll range is derived from the rows that are drawn, so the extra row is part
of the contract, not a rounding difference.

### The search functions under the new wrap

The plan's §14 stop condition — *"the search functions' behaviour changes under
the new wrap in a way that cannot be attributed"* — was **hit and attributed**.

- `highlight_match_in_rendered`'s threshold is the **first rendered row's**
  display width. The character-aware engine filled row 0 to exactly 60 columns;
  the word-aware engine stops at the word boundary at 52 columns. The
  threshold `m.end > total_width` therefore now trips about eight columns
  earlier, and more first-line matches are dropped.
- The **defect itself is unchanged**: the byte-offset-versus-display-column
  comparison is the same defect it was at the baseline.
- `scroll_to_current_search_match`'s **inputs** move with the transcript's row
  count; its **asserted behaviour** is unchanged.

Neither function was modified by this milestone; both defects are recorded in
§10 and tracked by the roadmap's risk R4.

### Failures recorded, not concealed

The following failures are **not** this milestone's failures, and are recorded
so that a reader is not left to discover them:

| Failure | Attribution |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** This host's `/tmp` root path is not usable by the daemon-refresh fixture. Nothing in this milestone touches the daemon. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` — `OfflineRequired is unavailable on this platform` | **Environmental and pre-existing.** The macOS host has no offline-required sandbox backend. Nothing in this milestone spawns a process. |
| (resolved) `tui::command::tests::command_docs_builtin_slash_action_variant_count_matches_source` and `tui::command::tests::command_docs_count_matches_registry` — stale counts in `architecture/command.md` ("claims BuiltinSlashAction has 160 variants; the enum has 161"; "count 168 drifts from registry 169") | **Not this milestone's.** The extra variant is M004's `/expand-all` slash action (`src/tui/command.rs`), and the stale counts were in `architecture/command.md`, which this milestone does not touch. **Documentation counts**, in the same class as the `overview.md` row below. The counts were corrected in `architecture/command.md` in the same working tree, and both tests now **pass**. |
| (resolved) `check_test_target_references` — the `architecture/overview.md` integration-test-file count row | **A documentation obligation, not a product defect.** The row records a count of test *files*; the guard compares it against the tree. It was the gate's one red step; the row has been re-measured and corrected in the same working tree, and the step is green. §9 names it. |
| (not reproduced) `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` | **Flaky, and not a failure of this milestone.** It failed once under the load of a full `--lib` run on this host, **passes when run in isolation** (`cargo test --locked -p codegg --lib tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` → 1 passed, 0 failed), and passed in the final full `--lib` run. It touches no TUI code. |

**Skipped scope.** `scripts/verify.sh full` (clippy `-D warnings` over the
whole workspace, the workspace test suite, and
`cargo test -p codegg --features server,plugins,lsp-test-support`) was **not
run**, and no claim is made about it. A hosted CI run was **not** executed
against this code; the evidence above is local. The change is confined to the
TUI's presentation path, and the commands above cover the modules it touches.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — one producer for rows and for the row count.** The wrap change lands inside the row producer; the scrollbar's count and the drawn rows continue to come from the same `Vec<Line>`. | The change is in `wrap_markdown_spans`, which is called from the renderer that produces the rows; `the_rows_the_transcript_draws_are_the_rows_the_row_count_reports` and `the_transcript_draws_the_rows_the_scrollbar_counts` | pass |
| **I2 — the TUI renders what the daemon sent.** No message part is synthesized, dropped or reordered. | The change is a wrapping strategy inside the row builder. No `MsgPart` construction, filter or reorder was added; the diff touches `src/tui/wrap.rs` and the markdown path in `src/tui/components/messages.rs` | pass |
| **I3 — the trust frame stays in the model path.** This milestone does not touch the model-facing path at all. | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; `src/search_backend/framing.rs` has **no diff lines** | pass |
| **I4 — nothing is silently unreachable.** No content becomes less reachable than it is today. | The wrap change alters where rows break, not which bytes are drawn. The measured row count **rose** by one per paragraph, and the scroll range is derived from the drawn rows, so the extra content is reachable by the same scroll the transcript already had | pass |
| **I5 — a wrap never breaks an escape sequence.** OSC-8 spans are excluded from word segmentation and from width measurement. | `wrap_styled_spans_by_word_keeps_an_osc8_span_whole`, `…does_not_count_escape_printable_parts_toward_the_width`, `…does_not_split_a_span_that_is_only_an_escape_sequence`, `visible_width_ignores_escape_sequences`, and the consumer-level `assistant_prose_keeps_an_osc8_auto_link_whole_across_a_wrap` | pass |

## 6. Failure and recovery review

- **Failure.** The primitive is total over its inputs: an empty source returns
  one empty row and a zero width returns the source unwrapped, so the row count
  is never zero for a non-empty buffer. No input makes it panic.
- **Duplicate delivery and idempotency.** Not applicable. The primitive is a
  pure function of `(spans, width)`; it has no durable state, so a repeated
  wrap of the same input produces the same rows.
- **Cancellation races.** Not applicable. Nothing asynchronous is added, and no
  cancellable operation is introduced.
- **Daemon or node restart.** Not applicable. Nothing is persisted, and the
  transcript's rows are rebuilt from the daemon's message parts on every
  layout.
- **Partial persistence failure.** Not applicable. No write path is added.
- **Stale generation or lease.** Not applicable. The primitive holds no
  generation counter and no lease.
- **Contention and resource release.** The primitive is frontend-local and
  single-threaded within a render. Its cost is O(source length) per layout —
  the same order as the per-character engine it replaces, and **3.5× cheaper in
  allocations** by the measurement in §4. There is no shared resource and
  nothing to release.
- **Malformed or unauthorized input.** A span carrying an escape sequence is
  handled by the primitive itself, not by a per-call-site special case: the
  exclusion is a property of the primitive, which is what the plan's §14 stop
  condition required. A malformed escape sequence that is not a recognised
  control sequence is measured as visible text, which is the safe direction —
  it can make a row break earlier, never later, so no content is clipped.
- **Bounded event and artifact behavior.** No event, artifact or file is
  produced. The row count is bounded by the source length and the width.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. No daemon message, session state, or wire format
  changes. The model-facing path is byte-identical (I3).
- **Configuration.** None. The wrap's behaviour is not user-configurable, and
  no configuration key is added, renamed or reinterpreted.
- **Backward compatibility.** The change is **user-visible but not
  incompatible**: model-authored prose now breaks at word boundaries instead of
  mid-word, and a paragraph may occupy one row more than it did. Every byte
  the baseline drew is still drawn; only the row boundaries move. The `read`
  tool's numbered output and fenced code blocks are unchanged, so code's
  layout is exactly what it was.
- **Rollback limitations.** The change is self-contained in one production
  module (`src/tui/wrap.rs`) plus the markdown call site in
  `src/tui/components/messages.rs`. Reverting those two files restores the
  previous behaviour exactly. There is no persisted state to migrate back, and
  no compatibility shim to unwind.
- **Legacy paths.** The character-aware per-character wrapper remains in the
  tree for the paths that still need it — fenced code blocks and the `read`
  tool's numbered output. It is a **retained** path, not a deprecated one: no
  caller was migrated away from it in this milestone.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no tool
  path, and no daemon interaction.
- **Secrets.** None handled. No credential, token, or path is read or logged.
- **Privilege boundaries.** Unchanged.
- **Path validation.** Not applicable — the milestone parses no paths and
  resolves none.
- **The trust frame.** The frame is a **prompt-injection defence** and it
  belongs to the model path. This milestone does not weaken it: it does not
  read the frame, does not parse it, and does not touch
  `src/search_backend/framing.rs`, which has **zero diff lines**. The model
  still receives the frame byte for byte.
- **Denial of service.** The primitive's work is bounded by the source length
  and the width, and it is **3.5× cheaper in allocation** than the engine it
  replaces. The greedy word placement is a single linear pass over the source;
  there is no backtracking and no quadratic search. The one pathological input
  — a single token wider than the row — is hard-broken at character
  boundaries, which is the same linear behaviour the plain engine already had.
- **Redaction.** Unchanged. The wrap renders bytes it is given; it adds no
  redaction and removes none.
- **Audit.** No new audit event. Text rendering is not an auditable action.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/tui.md` — "The styled, word-aware wrap": the two engines, the
    primitive's contract (whitespace breaking, style preservation, byte
    offsets, the atomic zero-width escape token), which paths use it, the
    call-site fallback rule, the measured effect, and the plain engine's
    continued existence.
  - `architecture/overview.md` — the integration-test-file count row, which
    this milestone's two new members moved.
- **Updated user-facing documentation.** None required: `docs/tui.md` does not
  describe the wrap's internal row breaking, and the plan's §12 made the
  `docs/tui.md` update conditional ("if it is described"). The user-visible
  change is that prose no longer breaks mid-word.
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the shared
  styled primitive, its properties, and the statement that code blocks and the
  `read` tool's output stay character-aware.
- **Static guards.** None added. The plan's work package A was satisfied by
  **tests**, not by a new guard, because the milestone adds no new call site
  for a guard to police. The existing guards — including
  `scripts/check_test_assertion_quality.py` and
  `scripts/check_test_duplication.py` — were run and pass.
- **Operator diagnostics.** None. Nothing to diagnose; a rendering change has
  no operational surface.
- **Documentation obligation — resolved.** The `architecture/overview.md`
  integration-test-file count row was the gate's one red step
  (`check_test_target_references`). It is a **count of test files**, not a
  behaviour; the row has been re-measured against the tree and corrected in
  the same working tree, and the step is green. Nothing is outstanding here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The transcript's search highlight is broken and the search scroll mixes a visual row offset with a logical line count.** `highlight_match_in_rendered` compares a byte offset against display columns, and `scroll_to_current_search_match` adds a visual row offset to a logical line count. | A search match in the main buffer can be missed or highlighted on the wrong column, and the scroll can land on the wrong line. The defect is **pre-existing** and unchanged by this milestone. | Recorded, **not fixed**: the roadmap places this in risk **R4**, and M001's scope is the primitive. The three `_known_defect` tests make the defect observable and name it. Tracked by the roadmap; a future milestone owns the fix. |
| medium | **The plain `wrap_to_strings` engine is still not escape-atomic.** A tool output that contains an escape sequence can still be split by the plain word wrap. | A tool output carrying a terminal control sequence can have that sequence split across a row boundary, which corrupts the sequence for the terminal. | **Pre-existing** and **outside this milestone's scope**: the baseline's character-aware wrapper had the same property, and the tool-output path was explicitly out of scope for M001. Recorded for a future milestone that moves the tool-output path onto the styled primitive. |
| low | **The primitive's bijection is over the rendered bytes, not over every byte of the buffer.** A whitespace run is collapsed to one space and the whitespace at a row break is dropped, so those bytes are on no cell. | A byte-level bijection over every byte is not expressible on top of this primitive without a second wrapping helper. Nothing that consumes the primitive today needs one. | Accepted and recorded. If a future consumer needs a byte-exact mapping, it needs a second helper rather than a change to this one. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the primitive
is in the tree, the tests are in the tree, and the measurement in §4 was taken
rather than estimated. The **outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA today),
   and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and the
hosted lane is a strict **subset** of `quick`, so a green hosted run does not
by itself prove everything the local gate proves.

M002 of this roadmap depends on M001's interface, and that dependency is
**discharged**: the primitive's signature is fixed and in the tree, and M002
built on it. The search defects stay recorded under the roadmap's risk R4 and
are not absorbed into this milestone.

## 12. Registry updates

The parent owns `plans/registry.md` and the roadmaps; the changes below are
**specified** here, and the parent has **already applied them** in the same
working tree. The "as applied" lines record what the files actually say now,
so a reader can check the registry against this record rather than against
this specification.

- `plans/registry.md` — the "TUI main buffer text rendering" row:
  - **Status** must read **conditionally closed**, and the row must name the
    baseline `f2639b5d2`.
  - The **milestone cell** must name this milestone's plan
    (`plans/implementation/tui-main-buffer-text-rendering/001-word-aware-wrapping-in-the-main-buffer.md`)
    **and** this closure record
    (`plans/closure/tui-main-buffer-text-rendering/001-status.md`), and must
    record the dependency and the class.
  - The **narrative** must record: the styled word-aware primitive in
    `src/tui/wrap.rs` and the measured **3.5x** reduction in per-layout slice
    allocation; that the plain word-aware engine **loses styles and byte
    offsets** and therefore cannot be used directly on styled spans; that the
    **OSC-8 hazard** is the one failure that would be hard to notice and easy
    to ship; that the two search functions are **covered and their defects
    recorded, not fixed**, under the roadmap's risk **R4**; that the row count
    and the scrollbar continue to come from **one producer**; and that no new
    static guard was added.
  - The **summary paragraph** must record the conditional closure and its
    named outstanding evidence — a closing commit plus a green hosted
    `CI / verify` run against it — and the two recorded findings.
- **As applied.** `plans/registry.md` carries a **workstream row** for
  "TUI main buffer text rendering" reading `**conditionally closed**`, and a
  **per-milestone table** whose M001 row reads `**conditionally closed**` and
  names both the implementation plan and this closure record, with "Depends
  on: nothing outstanding. Class: **capability** with an infrastructure
  obligation" and the baseline `f2639b5d2`. The roadmap's §13 "Milestone
  status" table reads `conditionally closed` for every milestone and carries
  the plan path and the closure path for each; the roadmap also gained an
  **"Outcome (2026-10-23)"** paragraph naming the line as **conditionally
  closed** and naming the outstanding item — the hosted `CI / verify` lane,
  which has not run against the commit that carries this code.

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md`:
  - §13 "Milestone status" must read `conditionally closed` for the M001 row,
    with the implementation-plan path and the closure-record path filled in.
    The **dependency cells are retained** rather than blanked — they are the
    record of what each milestone depended on — so the M002 row keeps
    `M001 (interface)`, the M003 row keeps `M001 (hard), M002 (interface)`,
    and the M004 row keeps `M003 (hard)`.
  - §11's **R4** row must state that the search defect survives this
    milestone and where it is tracked — which this closure record does in §10.
  - §12 "Completion definition" must record that the milestone's *production*
    completion is in the tree while its *operational* evidence (the closing
    commit and the hosted run) is outstanding.
  - The roadmap must carry an **"Outcome (2026-10-23)"** paragraph naming the
    line as conditionally closed, naming the two outstanding items, and
    recording the delivered capability.
