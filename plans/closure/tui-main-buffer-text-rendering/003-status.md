# TUI Main Buffer Text Rendering Milestone M003 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-main-buffer-text-rendering/003-collapsed-by-default-one-line-disclosure.md`

Source subsystem roadmap:

- `plans/subsystems/tui-main-buffer-text-rendering-roadmap.md#milestone-m003--collapsed-by-default-one-line-per-tool-call-and-per-reasoning-block`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are
  `src/tui/components/messages.rs` (the one-row header, the collapsed
  preview's suppression, the reasoning default),
  `src/tui/commands/sessions.rs` (the persisted session part),
  `src/tui/wrap.rs` (the ellipsis budget correction described below),
  `tests/tui_render/tui_main_buffer_collapsed_layout.inc`,
  `tests/tui_render/tui_main_buffer_row_accounting.inc` and
  `tests/tui_render/tui_main_buffer_row_precise_disclosure.inc`. No SHA,
  pull request number or commit message is claimed, because none exists: the
  implementing agents were not authorized to commit.

## 1. Executive finding

The milestone's capability boundary — **how many rows a tool call and a
reasoning block occupy in the main buffer** — is complete in the working tree,
and the completion is verified against the source rather than asserted.

At the baseline a collapsed tool call was **up to ten rows**: the header
wrapped, the preview showed the first lines of the output, and a
"(+N more lines)" hint sat under them. The disclosure machinery already
existed — `expanded: bool`, defaulting to `false` — but the *collapsed* form
was not a disclosure, it was a preview. A reasoning block was worse: its
`collapsed` flag defaulted to `false`, so it printed its whole body by
default.

What landed:

- The tool-call header is **one row**: `"  " + icon + " " + name` plus the
  **ellipsized target**, the summary, the hidden-output counter and the
  affordance. The ellipsis is applied **to the target only**; the name, the
  icon and the affordance are never truncated.
- The **preview rows and the "(+N more lines)" hint do not render while the
  call is collapsed**. A collapsed call with output is **one row**.
- The **hidden-output count moved into the header** as ` (+N lines)`, between
  the summary and the affordance, and is **dropped when expanded** (owner
  decision D5). The count comes from the **drawn body**, so it describes the
  content that *would* be drawn.
- `TOOL_TARGET_MAX_WIDTH` — the 400-column ceiling — was **removed**;
  `extract_tool_target` now returns the target untruncated and the row's
  remaining width is the only ceiling.
- **Reasoning blocks default to collapsed** at five construction sites.
- **Model prose is not collapsed** (owner decision D10).

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

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
| §13.1 — a collapsed tool call occupies exactly one row, at every width | the one-row-at-every-width unit test; `a_transcript_of_collapsed_calls_and_a_reasoning_block_counts_one_row_each` at widths 60 and 100 (`tests/tui_render/tui_main_buffer_collapsed_layout.inc`); the measured rendering probe at widths 20/40/60/80/120 reproduced in §4 | pass | **With the stated exception** in §4's deviations: a call that is still `Pending` or `Running` **with no output yet** draws its header **plus one status row**. |
| §13.2 — a collapsed reasoning block occupies exactly one row | the reasoning-defaults-to-collapsed unit test; `a_transcript_of_collapsed_calls_and_a_reasoning_block_counts_one_row_each` | pass | The reasoning body is reachable by expansion. |
| §13.3 — the one-line header shows the tool's icon, its name, an ellipsized target and a summary; the hidden-output count is in that row | the ellipsis-lands-on-the-target unit test; the counter-is-in-the-collapsed-header unit test; `src/tui/components/messages.rs:2335`–`:2339` | pass | The target's budget is `header_width.saturating_sub(fixed_width + 1)` (`:2329`) — the separator space is charged to the target. |
| §13.4 — an expanded tool call shows its content in full and its header without the hidden-output count | the counter-is-not-in-the-expanded-header unit test; the expanded-call-contributes-more-rows unit test; `a_collapsed_call_hides_its_output_and_the_expanded_call_shows_it` | pass | Owner decision D5: the count of hidden lines is noise once the content is visible. |
| §13.5 — model prose is **not** collapsed: an assistant answer occupies the rows it needs | the change touches only the tool-call header and the reasoning construction sites; `a_message_with_a_tool_call_a_reasoning_block_and_prose_renders_the_sum_of_its_parts_rows` (`tests/tui_render/tui_main_buffer_word_wrap.inc`) | pass | Owner decision D10. |
| §13.6 — the content of a collapsed tool call is reachable by clicking it | the collapsed-call-is-still-toggleable unit test (**I4 reachability**); `a_collapsed_call_hides_its_output_and_the_expanded_call_shows_it` | pass | This is the invariant the collapsed layout is judged against. |
| §15 — the header's and the row-count's unit tests | 11 unit tests in `src/tui/components/messages.rs` | pass | One row at every width; the ellipsis lands on the target and not on the name or the affordance; a row narrower than the tool name stays one row; the counter is in the collapsed header and not in the expanded one; the counter counts the content that would be drawn; an expanded call contributes more rows; a collapsed call is still toggleable and the expanded form renders; reasoning defaults to collapsed and renders one row; the thinking switch still gates the reasoning text; untrusted output is rendered as text and never as markup; a tool output with an escape sequence is not split. |
| §15 — a rendered transcript showing several collapsed tool calls, each on one row, at two widths | `a_transcript_of_collapsed_calls_and_a_reasoning_block_counts_one_row_each` at widths 60 and 100; the measured rendering probe at five widths in §4 | pass | The probe was a temporary harness and was removed after it was measured. |
| §15 — the scrollbar's row-count assertion (I1) | `the_transcript_draws_the_rows_the_scrollbar_counts` (`tests/tui_render/tui_main_buffer_collapsed_layout.inc`) | pass | — |
| §15 — the click-reachability assertion (I4) | the collapsed-call-is-still-toggleable unit test, and `a_collapsed_call_hides_its_output_and_the_expanded_call_shows_it` | pass | — |
| §15 — the disposition of the existing transcript tests that assert the old multi-row collapsed layout | §3's "Existing tests rewritten" subsection | pass | `test_messages_widget_toggle_reasoning` was rewritten; the three new integration members are new; no existing test was deleted. |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's verification table | **partial** | The gate is **green on the working tree** (exit 0, "Quick verification passed", 74 steps). What is missing is the other half of the item: **there is no closing commit**, so the gate has not been green *at a closing commit*, and the hosted `CI / verify` lane has not run against this code. This is the milestone's condition; see §1 and §11. |
| **Non-vacuity**: the new tests detect the absence of the change | with the two production files stashed and the tests in place, **3** of the new integration tests fail — the row-count test (`left: 22, right: 7`, the baseline's collapsed call is ten rows), the reachability test, and the rewritten `test_messages_widget_toggle_reasoning` | pass | The measurement is a property of the milestone's own tests, taken against the baseline rendering path. |
| The name, the icon and the affordance are never truncated | the ellipsis-lands-on-the-target unit test; a row narrower than the tool name stays one row | pass | The plan forbids clipping the affordance. |
| The summary yields rather than clipping the affordance | the summary-yields-at-narrow-widths deviation in §4 | pass (by decision) | Recorded as a deviation, not a silent choice. |
| Untrusted output is never rendered as markup | `an_untrusted_tool_output_is_rendered_as_text_and_never_as_a_command` | pass | The tool output is drawn as text, never interpreted. |
| A tool output carrying an escape sequence is not split | `a_tool_output_with_an_escape_sequence_is_not_split` | pass | The **plain** engine still splits at character boundaries; what is asserted is that the escape sequence itself is not cut. See §10 for the pre-existing limitation this leaves open. |

## 3. Production implementation evidence

**The one-row header (`src/tui/components/messages.rs`).**

- The header is assembled as `"  " + icon + " " + name`, then the **ellipsized
  target** (`:2335`–`:2339`), then the summary, then the hidden-output
  counter, then the affordance.
- The target's budget is `header_width.saturating_sub(fixed_width + 1)`
  (`:2329`); the separator space is charged to the target, so a target with no
  room left is no target at all rather than a truncated one.
- `extract_tool_target` (`:445`) now returns the target **untruncated** — the
  caller's row width is the only ceiling. The `TOOL_TARGET_MAX_WIDTH`
  400-column ceiling was **removed** from the tree; the doc comment at
  `:440`–`:444` records why: a target elided to a fixed column count was
  amputated mid-word with nothing indicating it had been cut, and the user
  could not tell what was running — which matters most precisely when a call
  is stuck and the command is the only diagnostic clue.

**The collapsed preview.**

- While the call is collapsed, the **preview rows and the "(+N more lines)"
  hint do not render**. A collapsed call with output is one row.
- The hidden-output **count moved into the header** as ` (+N lines)` between
  the summary and the affordance (`:2282`–`:2286`), and is **dropped when
  expanded**.
- The count's source is the **drawn body** — `output_line_count` from M002
  (`:2272`–`:2275`) — so it reflects the content that *would* be drawn, and a
  one-line body says `(+1 lines)`.
- The daemon-side summary's `output_lines` entry is **suppressed while
  collapsed**, so a row never shows two line counts.

**Reasoning defaults to collapsed** at five production construction sites:

- `src/tui/components/messages.rs:736` — the streaming tag parser's
  end-of-block branch;
- `src/tui/components/messages.rs:752` — the parser's still-thinking branch;
- `src/tui/components/messages.rs:805` — `add_reasoning`'s append path;
- `src/tui/components/messages.rs:821` — `add_reasoning`'s new-message path;
- `src/tui/commands/sessions.rs:1701` — `transcript_part`, the persisted
  session part.

Two **test** fixtures deliberately keep `collapsed: false`
(`src/tui/components/messages.rs:4312` and `:5700`) because they exercise the
expanded reasoning body.

**The parent correction made after this milestone.** M003's implementer found,
and reported, a defect in the shared helper it consumed: `ellipsize_to_width`
in `src/tui/wrap.rs` reserved the ellipsis's **UTF-8 byte length**
(`ELLIPSIS.len_utf8()` = 3) as a **column** budget, while `…` is one column
wide. Every ellipsized string therefore came out up to **two columns
narrower** than the budget it was handed — which directly shortened this
milestone's header target. The **parent** corrected it: the budget now reserves
the ellipsis's **display width** (`src/tui/wrap.rs:774`), and a new unit test
`ellipsize_fills_its_column_budget` (`:907`) asserts that the output's display
width equals the requested budget.

**Tests.**

- `src/tui/components/messages.rs` — 11 unit tests.
- `tests/tui_render/tui_main_buffer_collapsed_layout.inc` — 5 integration
  tests: a transcript of collapsed calls and a reasoning block counts one row
  each at widths 60 and 100; the transcript draws the rows the scrollbar
  counts; a collapsed call hides its output and the expanded call shows it;
  untrusted output is never a command; an escape sequence is not split.

**Existing tests rewritten.**

- `test_messages_widget_toggle_reasoning` was **rewritten**: the reasoning
  block now defaults to collapsed, so the test that toggled it had to be
  written against the new default. It is one of the three tests that fail
  against the baseline, which is what makes it non-vacuous.
- The three integration members are **new**. No existing transcript test was
  deleted.

**Documentation.** The plan's §12 required `architecture/tui.md` to document
"the collapsed layout, the header's shape, the ellipsis rule, and the
explicit statement that model prose is not collapsed", and `docs/tui.md` to
document "the collapsed-by-default behaviour and how to expand". **All three
landed**, in the working tree this record reviews:

- `architecture/tui.md` — "The disclosure model: collapsed by default": the
  per-**part** disclosure state (`MsgPart::ToolCall { expanded }` and
  `MsgPart::Reasoning { collapsed }`), the statement that **model prose is not
  a disclosure at all**, the one-row collapsed tool call, the header's shape
  and the ellipsis landing on the target alone, the removal of the
  `TOOL_TARGET_MAX_WIDTH` ceiling, and the hidden-output count's move into the
  header.
- `docs/tui.md` — "Reading the transcript → What is collapsed by default": the
  collapsed-by-default behaviour in user-facing terms, the `(+N lines)` count,
  the `read` tool's exception, the second line a call with no output yet
  draws, and the narrow-terminal rule that the row gives up the timing rather
  than the name.
- `.opencode/skills/tui/SKILL.md` — the module guide's statement that the
  transcript's disclosures are collapsed by default, that a collapsed tool
  call is one row, that the counter is dropped when expanded and counted from
  the drawn body, that the daemon-side `output_lines` is suppressed while
  collapsed, that `collapsed: true` is set at all five construction sites,
  and that **model prose is never collapsed**.

## 4. Verification executed

### Commands run

```bash
# recorded at this milestone's own verification point
cargo test --locked -p codegg --lib tui::components::messages
cargo test --locked -p codegg --test tui_render
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo check --workspace --all-targets --locked
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::messages` | **74 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1048 passed**, 0 failed |
| `python3 scripts/check_test_assertion_quality.py` | all checks pass |
| `python3 scripts/check_test_duplication.py` | all checks pass |
| `cargo check --workspace --all-targets --locked` | exit 0 |
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

### The measured rendering (probe removed afterwards)

```
W120  ✓ bash cargo test -p codegg --all-targets --locked (1ms, exit 0) (+4 lines) expand
W80   ✓ bash cargo test -p codegg --all-targets -… (1ms, exit 0) (+4 lines) expand
W60   ✓ bash cargo test -p co… (1ms, exit 0) (+4 lines) expand
W40   ✓ bash cargo test… (+4 lines) expand
W20   ✓ bash (+4 lines) expand
```

Every width from 20 to 120 draws a collapsed tool call as **one row**, and the
affordance is present at every one of them.

### Re-verification (this record's author, on the same working tree)

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::components::messages` | **80 passed**, 0 failed — **higher** than the 74 recorded above, because M004 of this same roadmap added unit tests to the same module after this milestone's verification point |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed — **higher** than the 1048 recorded above, for the same reason |
| `cargo test --locked -p codegg --lib` | **7373 passed, 3 failed** — the three environmental failures in the table below, and nothing else. |
| `bash scripts/verify.sh quick` | **exit 0 — green**: "Quick verification passed", 74 steps. The one red step seen earlier in this record's writing — `check_test_target_references`, "architecture/overview.md records 326 integration test files, but the tree holds 327 root integration test modules" — was a **stale documentation count**, re-measured and corrected in the same working tree. |

The recorded test counts are **snapshots of a tree that other agents were
concurrently modifying** — the main-buffer and composer milestones of this line
were implemented in parallel, and each milestone's integration members landed in
the same family. They are therefore **not a monotonic series** across the five
records of this line, and each record's count describes the tree at that
milestone's own verification point.

### Deviations, and the plan wording they resolve

1. **The pending/running status row is kept.** A call that is still `Pending`
   or `Running` **with no output yet** draws its header **plus one status
   row** ("queued — waiting on your approval · Enter to review" / "queued —
   not started yet (waiting on admission or approval)" / "running — waiting
   for command/tool output..."). Plan §6 suppresses exactly two things — the
   preview rows and the "(+N more lines)" hint — and the status row is
   neither; the code's own comment calls it "the backup way back into it" for
   a dismissed approval, and dropping it would regress I4 and the
   admission-versus-running diagnosis. **The one-row promise therefore holds
   for a call *with* output**, which is the case the plan measures; a call in
   flight is a materially different state and keeps its live status line.
2. **The summary yields at narrow widths.** When the fixed parts do not fit,
   the summary (duration/exit code) is dropped so the name, the counter and the
   affordance survive — the plan forbids clipping the affordance.
3. **`show_thinking` semantics left as shipped — a resolution, not a defect.**
   `show_thinking = false` hides the reasoning **body** but still draws the
   one-line `Thinking` header. The plan's §6 wording ("still hides reasoning
   entirely") is the imprecise one: the **owner's own instruction** was
   "thinking should be hidden by default as well so just 'thinking' on one
   line". The shipped behaviour matches the owner. This is recorded as a
   **resolution of the plan wording in the owner's favour**.
4. **I5 test scope.** The tool-output body still uses the plain word-wrap
   engine, which is not escape-atomic; switching it to the styled engine is
   outside this milestone and would change every tool output's wrapping. The
   integration test asserts what is true — the escape sequence is not split —
   and the pre-existing limitation is recorded in §10.
5. **The "(+N lines)" count is logical lines, not drawn rows** — inherited
   from M002's `output_line_count`, unchanged.

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
| **I1 — one producer for rows and for the row count.** | The change is in the row builder that produces the header rows; `the_transcript_draws_the_rows_the_scrollbar_counts` and `the_rows_the_transcript_draws_are_the_rows_the_row_count_reports` | pass |
| **I2 — the TUI renders what the daemon sent.** | The change alters how a part is **laid out**, not what is sent. The one place the milestone reaches outside the TUI — `src/tui/commands/sessions.rs`'s `transcript_part` — sets the **display** flag `collapsed: true` on the persisted session part, which is a rendering decision, not a change to the daemon's message. No message part is synthesized, dropped or reordered. | pass |
| **I3 — the trust frame stays in the model path.** Unchanged; M002 owns it. | `git diff f2639b5d2 -- src/search_backend/` is **zero lines**; `src/search_backend/framing.rs` has **no diff lines** | pass |
| **I4 — nothing is silently unreachable.** The collapsed view shows less; the content must remain reachable by expansion. | The collapsed-call-is-still-toggleable unit test (**the I4 reachability assertion**) and `a_collapsed_call_hides_its_output_and_the_expanded_call_shows_it`; the pending/running status row is deliberately kept so a dismissed approval's call stays findable | pass |
| **I5 — a wrap never breaks an escape sequence.** | `a_tool_output_with_an_escape_sequence_is_not_split`; the escape-atomicity of the **styled** primitive is M001's, and its tests are green in the same tree. The **plain** engine's gap is pre-existing and is recorded in §10. | pass |
| **I6 — model prose is not collapsed.** Owner decision D10: only tool calls and reasoning blocks collapse. An assistant answer may occupy as many rows as it needs. | The change touches only the tool-call header and the reasoning construction sites; `a_message_with_a_tool_call_a_reasoning_block_and_prose_renders_the_sum_of_its_parts_rows` | pass |

## 6. Failure and recovery review

- **Failure.** The header's arithmetic uses saturating subtraction
  (`header_width.saturating_sub(fixed_width + 1)`), so a row narrower than the
  tool's own name cannot underflow: the target is dropped and the name, the
  icon and the affordance survive. A row narrower than the tool name is still
  **one row**.
- **Duplicate delivery and idempotency.** Not applicable. The header is
  rendered from the message parts the widget already holds; rendering it twice
  produces the same row.
- **Cancellation races.** Not applicable. Nothing asynchronous is added.
- **Daemon or node restart.** Not applicable. Nothing is persisted by this
  milestone; `src/tui/commands/sessions.rs`'s `transcript_part` reconstructs a
  part from the **persisted session**, and its `collapsed: true` is a display
  default applied on every load — a restart cannot leave a half-collapsed
  state, because there is no half-collapsed state.
- **Partial persistence failure.** Not applicable. No write path is added.
- **Stale generation or lease.** Not applicable. The collapsed flag is
  per-part display state, rebuilt with the rows.
- **Contention and resource release.** The header is frontend-local and
  single-threaded within a render. It is strictly **cheaper** than the
  baseline's rendering: one row instead of up to ten, and the ellipsized
  target is computed once per header. There is no shared resource and nothing
  to release.
- **Malformed or unauthorized input.** The header renders the tool's **name**
  and a **target** the widget already holds. The target is passed through
  `crate::tui::wrap::normalize_inline` (`src/tui/components/messages.rs:454`,
  `:489`), which is the pre-existing normalization for inline text. The
  milestone adds no new parsing of model-supplied content and no new
  interpretation of it.
- **Bounded event and artifact behavior.** No event or artifact is produced.
  The header is one row; the count it carries is the count of the drawn body.

## 7. Migration and compatibility review

- **Schema migration.** None. No database schema is touched.
- **Protocol.** None. No daemon message, session state, or wire format
  changes. The model-facing path is byte-identical (I3).
- **Configuration.** None. No configuration key is added, renamed or
  reinterpreted. The `show_thinking` switch keeps its meaning; the milestone
  resolves its **wording** against the owner's instruction rather than
  changing its behaviour.
- **Backward compatibility.** The change is **user-visible**. A collapsed tool
  call that used to occupy up to ten rows now occupies **one**; a reasoning
  block that used to print its whole body now prints one row. The content is
  **not lost** — it is one click away, and the header carries the count of
  what is hidden. The `TOOL_TARGET_MAX_WIDTH` ceiling's removal means a long
  target is now ellipsized to the row's actual width rather than to a fixed
  400 columns, which is a visible change for a very wide terminal and an
  improvement in both.
- **Rollback limitations.** The change is self-contained in
  `src/tui/components/messages.rs`, `src/tui/commands/sessions.rs` and — for
  the parent's ellipsis correction — `src/tui/wrap.rs`. Reverting them
  restores the previous behaviour exactly. There is no persisted state to
  migrate back. One caveat: reverting `src/tui/wrap.rs`'s ellipsis correction
  would restore a **two-column** shortfall in every ellipsized string in the
  TUI, which is why it should be reverted only with the milestone.
- **Legacy paths.** The **pending/running status row** is retained, not
  removed — a call in flight keeps its live status line. The **preview rows**
  are still rendered when the call is **expanded**; the milestone suppresses
  them only while the call is collapsed.

## 8. Security review

- **Authorization.** Unchanged. This milestone adds no key handling, no tool
  path, and no daemon interaction.
- **Secrets.** None handled. No credential, token, or path is read or logged.
- **Privilege boundaries.** Unchanged.
- **Path validation.** Not applicable — the milestone parses no paths and
  resolves none. The tool's target is the one the tool already received and
  already displayed at the baseline.
- **The trust frame — the security statement this milestone must make.** The
  trust frame is a **prompt-injection defence** and this milestone does not
  weaken it: the milestone does not read the frame, does not parse it, and
  does not touch `src/search_backend/framing.rs`, which has **zero diff
  lines**. The model still receives the frame byte for byte. What the
  milestone changes is the **collapsed header's** content: the header shows
  the tool's name, an ellipsized target and a count, never the tool's
  untrusted output — and the integration test
  `an_untrusted_tool_output_is_rendered_as_text_and_never_as_a_command`
  asserts that untrusted output is drawn as **text** and never as markup, so
  a tool result cannot become a command or an escape sequence in the
  transcript.
- **Denial of service.** The milestone **reduces** the work on the render
  path: one row per collapsed call instead of up to ten, and one ellipsization
  per header instead of a wrapped multi-line preview. The header's arithmetic
  is O(1) in the target's length apart from the ellipsization itself, which
  is bounded by the row's width.
- **Redaction.** Unchanged. The milestone adds no redaction and removes none.
- **Audit.** No new audit event. Rendering a tool call's header is not an
  auditable action.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/overview.md` — the integration-test-file count row, which
    this milestone's new member moved.
  - `architecture/tui.md` — "The disclosure model: collapsed by default": the
    per-**part** disclosure state, the statement that **model prose is not a
    disclosure at all**, the one-row collapsed tool call, the header's shape
    and the ellipsis landing on the target alone, the removal of the
    `TOOL_TARGET_MAX_WIDTH` ceiling, and the hidden-output count's move into
    the header.
- **Updated user-facing documentation.** `docs/tui.md` — "Reading the
  transcript → What is collapsed by default": the collapsed-by-default
  behaviour in user-facing terms, the `(+N lines)` count and where it comes
  from, the `read` tool's exception, the second line a call with no output yet
  draws, and the narrow-terminal rule that the row gives up the timing rather
  than the name.
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the module
  guide's contract for the collapsed layout: the disclosures are collapsed by
  default, a collapsed tool call is one row, the counter is dropped when
  expanded and counted from the drawn body, the daemon-side `output_lines` is
  suppressed while collapsed, `collapsed: true` is set at all five
  construction sites, and **model prose is never collapsed**.
- **Static guards.** None added. The milestone adds no new call site for a
  guard to police — the header is assembled inside one module. The existing
  guards were run and pass, including
  `scripts/check_test_assertion_quality.py` and
  `scripts/check_test_duplication.py`.
- **Operator diagnostics.** None. Nothing to diagnose.
- **Documentation obligation — resolved.** The `architecture/overview.md`
  integration-test-file count row was the gate's one red step
  (`check_test_target_references`) — a **documentation count**, not a
  behaviour. The row has been re-measured against the tree and corrected in
  the same working tree, and the step is green. The plan's §12 documentation
  for this milestone **landed** in the working tree this record reviews.
  Nothing is outstanding here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The plain `wrap_to_strings` engine is still not escape-atomic**, so a tool output that contains an escape sequence can still be split by the plain word wrap that the tool-output body uses. | A tool output carrying a terminal control sequence can have that sequence split across a row boundary, which corrupts the sequence for the terminal. | **Pre-existing** and **outside this milestone's scope**: switching the tool-output body to the styled engine would change every tool output's wrapping. Recorded for a future milestone that moves the tool-output path onto the styled primitive. |
| low | **A collapsed call that is still `Pending` or `Running` with no output yet draws two rows, not one.** | The "one row per tool call" promise holds for a call *with* output — the case the plan measures — and not for a call in flight. | **Accepted and recorded as a deviation** (see §4): the status row is the backup way back into a dismissed approval, and dropping it would regress I4 and the admission-versus-running diagnosis. Not to be "fixed" without the owner's decision. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the one-row
header, the collapsed preview's suppression, the reasoning default and the
removed target ceiling are in the tree; the tests are in the tree; and the
non-vacuity measurement in §2 (3 of the new integration tests fail against the
baseline rendering path) shows the tests detect the absence of the change. The
**outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA today),
   and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and the
hosted lane is a strict **subset** of `quick`, so a green hosted run does not
by itself prove everything the local gate proves.

No documentation obligation remains outstanding: the
`architecture/overview.md` count row was re-measured and corrected in the same
working tree, and the plan's §12 documentation for this milestone **landed**.
The only outstanding evidence is the operational pair named above — the
closing commit and the hosted run.

M004 of this roadmap depends on M003's disclosure model, and that dependency
is **discharged**: the collapsed-by-default layout and the one-row header are
in the tree.

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
    (`plans/implementation/tui-main-buffer-text-rendering/003-collapsed-by-default-one-line-disclosure.md`)
    **and** this closure record
    (`plans/closure/tui-main-buffer-text-rendering/003-status.md`), with the
    dependency `M001 (hard), M002 (interface)` and the class.
  - The **narrative** must record: that the disclosure machinery **already
    exists** and the milestone changes **defaults and layout**, not the model;
    that a collapsed tool call was **up to ten rows** and is now **one**; that
    the one-line header must **ellipsize the target**, not the whole header,
    because a whole-header ellipsis eats the expand affordance; that the
    hidden-output count must **move into** the row and be **dropped** when
    expanded; that **model prose is not collapsed** (owner decision); and that
    the click target stays as it is until the next milestone, so the collapsed
    layout must not remove the last route to a tool call's content.
  - The **summary paragraph** must record the conditional closure and its
    named outstanding evidence, the two **recorded deviations** — the retained
    pending/running status row and the summary yielding at narrow widths —
    and the **resolution** of the plan's "hides reasoning entirely" wording
    in the owner's favour.
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
  - §13 "Milestone status" must read `conditionally closed` for the M003 row,
    with the implementation-plan path and the closure-record path filled in.
    The **dependency cells are retained** rather than blanked.
  - §5.3 "the collapsed tool call is up to ten rows, and the header wraps"
    must be updated to record that the collapsed call is now **one row** and
    that the target's 400-column ceiling was removed.
  - §5.3's record of the click-target defect must be updated: the milestone
    asserts the collapsed call is still **toggleable and reachable**, so the
    roadmap's record of that defect must say what remains.
  - §12 "Completion definition" must record that the milestone's *production*
    completion is in the tree while its *operational* evidence (the closing
    commit and the hosted run) is outstanding.
  - The roadmap must carry an **"Outcome (2026-10-23)"** paragraph naming the
    line as conditionally closed, naming the two outstanding items, and
    recording the delivered capability.
