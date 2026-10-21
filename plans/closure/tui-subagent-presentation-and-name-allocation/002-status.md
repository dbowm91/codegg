# TUI Subagent Presentation and Name Allocation — Milestone P002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-subagent-presentation-and-name-allocation/002-tab-bar-identity-persona-and-hints.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P002)

Repository baseline reviewed: `31a412844fbbf36055b29245075121035eefc385`

Implementation commits or pull requests:

- Implementation commit `682c6999d` (`feat(tui): the subagent presentation and
  name-allocation workstream — P001–P004`) on branch
  `feat/auto-20261020-3702c7c5`.
- Pull request [#150](https://github.com/dbowm91/codegg/pull/150). The branch was
  reconciled with `main` at `2a0bae08f` in merge commit `fffd96426`, with no
  conflicts, and every verification result in this record was re-taken on the
  merged tree.
- The change is confined to `src/tui/app/state/subagent_tabs.rs`,
  `src/tui/app/state/mod.rs`, `src/tui/app/render.rs`,
  `src/tui/components/sidebar.rs`, `src/tui/input.rs`,
  `tests/tui_main_buffer.rs`, `docs/tui.md`, `architecture/tui.md`, and
  `.opencode/skills/tui/SKILL.md`, plus the plan records named above.

## 1. Executive finding

The subagent tab strip was a row of **names** and nothing else. The main
conversation's entry read `Chat` with no marker of any kind, so the row did not
read as a strip of tabs; the keys that operate it (`⇧P` / `⇧N` / `⇧X`) were
documented in `docs/tui.md` and nowhere on screen; the subagent's own
description of itself was on the **side panel's** row instead, where a one-line
column spent on prose pushes the three facts a reader scans for — name, agent
type, status — to the left. The strip was also the one surface in the TUI whose
width arithmetic did not match what it painted: `bar_width` counted the labels
and the separators but **not** the status glyphs, so a bar that fitted on paper
painted a marker past the width it had promised.

The strip is now the identity surface it was meant to be, and it says so on one
row:

- the main entry reads **`Main`** and wears **`▣`**, a marker from a different
  shape family than the three liveness glyphs, because the main conversation is
  not a run and must not be mistaken for one;
- the **active** tab's **persona** is on the row, in italics, directly after the
  entry it describes — the projection's own value, carried on the row the tab
  already holds;
- the row states **`⇧P/⇧N switch · ⇧X close`** on the right, on the row it
  already occupies;
- the side panel's subagent row is now **name, agent type, status** and no
  prose.

All four share **one width budget**, computed once in `plan_strip`
(`src/tui/app/state/subagent_tabs.rs:696`) in a fixed precedence: the **hints**
are reserved first and in full, the **labels** keep their text (oldest tabs are
dropped with an ellipsis rather than a label truncated), and the **persona**
takes what is left and is dropped rather than squeezed below
`MIN_STRIP_PERSONA_COLS`. The glyph is now counted in that arithmetic, which is
what stops a marker from painting under the hints.

**Two of the plan's premises needed correcting against source, and both
corrections are recorded rather than absorbed silently.** The plan's §5
precedence list and its §8 degradation order disagree with each other (§3.1
deviation 1), and the plan's §4 assumed the strip's width arithmetic already
counted the glyph when it did not (§3.1 deviation 4).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AC1 — the main entry reads `Main` and carries a glyph distinct from `●`/`○`/`✕` | `MAIN_TAB_LABEL` (`subagent_tabs.rs:62`), `MAIN_TAB_GLYPH` (`:75`); `bar_tabs()` (`:430`); unit test `the_main_entry_reads_main_and_carries_a_glyph_of_its_own`; render test `the_tab_bar_marks_the_main_entry_as_a_tab` | pass | `▣` is a square, not a circle or a cross; `the_main_glyph_costs_exactly_one_column` asserts its measured width is 1 |
| AC2 — the active tab's persona is on the strip, in italics, truncated or dropped | `SubagentBarTab::persona` (`:480`), `plan_strip` (`:696`), `fit_persona` (`:732`); render test `the_tab_bar_shows_the_active_tabs_persona_in_italics`; unit test `the_persona_is_truncated_to_the_width_that_is_left` | pass | the persona is the projection's `profile`, read through `SubagentRow::descriptor()` |
| AC3 — with the main entry active, no persona is shown | `bar_tabs()` (`:430`) sets `persona: None` for the main entry; render test `the_tab_bar_shows_no_persona_while_the_conversation_is_the_active_tab`; unit test `the_main_entry_is_active_shows_no_persona` | pass | |
| AC4 — `⇧P`, `⇧N`, `⇧X` are visible on the strip row whenever a tab is open, absent when none is | `BAR_HINTS` (`:94`), rendered in `render_subagent_tab_strip` (`render.rs:1765`); render test `the_tab_bar_names_main_and_states_the_keys_that_operate_it`; negative test `no_tabs_means_no_strip_row_and_no_hints` | pass | the row exists only while tabs are open, so the hints cannot outlive what they describe |
| AC5 — no new row is introduced anywhere; the transcript does not reflow | the hints are drawn inside the existing strip row; `subagent_tab_strip_height()` is unchanged (`src/tui/app/mod.rs:15113`); render test `the_tab_bar_keeps_its_content_on_its_own_row` | pass | the plan's rejected alternative (the composer hint line) was not used — that row is reserved only when it is present, so writing there would reserve a **new** row |
| AC6 — the side-panel subagent row shows name, agent type and status, and no prose | the row block (`src/tui/components/sidebar.rs:1409`); unit test `a_subagent_row_shows_its_name_agent_type_and_status`; render test `a_rendered_subagent_row_shows_the_facts_and_not_the_prose` | pass | `MIN_SIDEBAR_DESCRIPTOR_CHARS` and the descriptor append were removed, not repurposed |
| AC7 — the pool-isolation guard is green | `python3 scripts/check_tui_subagent_name_pool_isolation.py` and its `--self-test` | pass | the persona is the projection's value; the TUI never resolves one against the pool |

## 3. Production implementation evidence

| File | Change | Anchor |
|---|---|---|
| `src/tui/app/state/subagent_tabs.rs` | `MAIN_TAB_LABEL` → `Main`; new `MAIN_TAB_GLYPH`, `BAR_HINTS`, `BAR_GAP`, `MIN_STRIP_PERSONA_COLS` | `:62`, `:75`, `:94`, `:97`, `:105` |
| | `SubagentBarTab` gains `persona: Option<String>`, and `glyph_width` / `entry_width` so a painted glyph is a measured glyph | `:465`, `:497`, `:508` |
| | `bar_width` counts the glyph; the degenerate clamp in `fit_bar_tabs` drops a glyph whose label was squeezed away | `:519`, `:636` |
| | new `StripPlan` and `plan_strip` — the one width budget, in one place | `:672`, `:696` |
| | new `fit_persona` — prose cut with an ellipsis, control characters flattened | `:732` |
| | `bar_tabs()` carries the **active** entry's persona and the main entry's glyph | `:430` |
| `src/tui/app/state/mod.rs` | re-exports the new items | `:79` |
| `src/tui/app/render.rs` | `render_subagent_tab_strip` renders the plan: entries and persona on the left, hints right-aligned on the right | `:1699` |
| `src/tui/components/sidebar.rs` | the subagent row drops the descriptor; `MIN_SIDEBAR_DESCRIPTOR_CHARS` removed | `:1409`, `:196` |
| `src/tui/input.rs` | the `⇧N` doc comment names `Main`, not `Chat` | `:242` |
| `docs/tui.md` | §"Subagent tabs" rewritten: the strip's content, the hints, the budget, the persona's home | `:728` |
| `architecture/tui.md` | §"One row, five surfaces" rewritten: the sidebar row is a scan surface, the persona lives on the strip, and the budget's precedence | `:759` |
| `.opencode/skills/tui/SKILL.md` | two new bullets: the strip is the identity surface and its row is one width budget; the sidebar row is a scan surface | `:1024` |

### 3.1 Deviations from the plan, and why

1. **The plan's §5 precedence list and its §8 degradation order contradict each
   other.** §5 ranks the consumers — hints, then labels, then "the persona:
   what remains after 1 and 2" — while §8 says a crowded row must degrade "by
   dropping tabs … and then the persona", which puts the persona *above* the
   tabs. The two cannot both hold. §10's required test is the only normative
   statement of the rule, and it says **"labels are preferred over the
   persona"**; that is what was implemented, and §8 is read as its weaker
   claim — the row degrades without overflowing. The consequence is recorded
   here rather than buried: on an 80-column row the persona is visible up to
   about four open subagent tabs and dropped beyond that, because the hints
   take 23 columns and the labels take the rest.
2. **The glyph was moved to the front of the label** (`▣ Main`, `● Sally`
   rather than `Main ▣`, `Sally ●`). The plan fixed the main entry's label and
   the fact that it carries a glyph, but not the position. A trailing marker
   after `Main` reads as a trailing *action*; a leading marker reads as that
   name's status, and the main entry's marker reads as a tab marker. The
   position was unified across the whole row rather than applied to the main
   entry alone, so the row carries one convention.
3. **The main entry's glyph is `▣` (U+25A3), not a liveness glyph.** The three
   liveness glyphs encode *a run's state*; the main conversation is not a run
   and has no state to encode, so wearing one of them would be a lie the user
   has to decode. A square is a different shape family from a circle and a
   cross, and `▣` is used nowhere else in the TUI, so it cannot collide with an
   unrelated meaning. `the_main_glyph_costs_exactly_one_column` asserts its
   measured width is 1, because a two-column marker would silently break the
   budget.
4. **The strip's width arithmetic did not count the glyph, and the plan
   assumed it did.** §4 required the budget to count the glyph; `bar_width`
   summed `label_width` and the separators and nothing else, so every glyphed
   entry painted two columns past the width the budget had granted it. That
   was harmless while the strip was the only thing on the row; with the hints
   on the same row it would have put a marker under the hints. `entry_width`
   now counts the glyph as it is painted, and the degenerate clamp drops a
   glyph whose label was squeezed away — a marker next to no name says nothing
   and is the only thing left that can give a column back.
5. **The persona sits directly after the entry it describes, not at the end of
   the row.** The plan fixed the persona's *content* and its style but not its
   position. At the end of the row it reads as a description of the **last**
   tab, which is wrong whenever the active tab is not the last one. Inserting
   it after the active entry costs nothing in the budget: the persona is
   exactly what the labels left behind, so the row's total width is unchanged.
6. **`MIN_SIDEBAR_DESCRIPTOR_CHARS` was removed rather than repurposed.** §7
   work package C named it as the floor under which the side-panel descriptor
   is dropped. The row no longer carries prose at all, so there is nothing to
   drop and the constant has no referent. The strip's own floor,
   `MIN_STRIP_PERSONA_COLS`, is the same number (12) for the same reason: a
   persona squeezed to a couple of characters reads as a rendering fault rather
   than as prose. This is a removal of a `pub` item.
7. **The gap between the entries and the hints only exists when the row has
   room for it.** A row narrower than the hints themselves gives its last
   column to the hints rather than to whitespace; the budget and the renderer
   agree on the geometry, and the unit test
   `a_strip_too_narrow_for_the_hints_keeps_the_row_inside_its_own_width` walks
   widths 0–40 and asserts the row is never over-painted.
8. **The hint wording is the plan's suggested shape verbatim** —
   `⇧P/⇧N switch · ⇧X close`, 23 display columns. The plan allowed other
   wording as long as all three keys are present and recognisable, and the
   suggested shape is the one the plan's own acceptance criteria can be read
   against.

## 4. Verification executed

### 4.1 Commands run

```bash
# narrow tests
cargo test -p codegg --lib tui::app::state::subagent_tabs
cargo test -p codegg --lib tui::components::sidebar
cargo test -p codegg --lib tui::app::render
cargo test -p codegg --lib tui::
cargo test -p codegg --test tui_main_buffer
cargo test -p codegg --test tui_render
cargo test -p codegg --test tui_side_panels

# static guards
python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test
python3 scripts/check_tui_subagent_name_pool_isolation.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the broader sweep
scripts/verify.sh quick
```

### 4.2 Baseline-failing evidence (non-vacuity)

The pre-P002 **production** state was reconstructed with the new tests kept
(`subagent_tabs.rs` and `state/mod.rs` restored to the baseline, `sidebar.rs`
and `render.rs` surgically reverted), and the new tests were run against it.
The strip rendered as:

```
"│Chat │ Sally ●                                                       │ Session                    │"
```

which is the defect in one line: the entry is `Chat`, it has no marker, the
keys are not stated, and the persona is nowhere on the row.

| Test | Result at the pre-P002 state |
|---|---|
| `the_tab_bar_names_main_and_states_the_keys_that_operate_it` | **FAILED** — `the main entry is named: "│Chat │ Sally ● …"` |
| `the_tab_bar_marks_the_main_entry_as_a_tab` | **FAILED** — `the main entry must read as a tab: "│Chat │ Sally ● …"` |
| `the_tab_bar_shows_the_active_tabs_persona_in_italics` | **FAILED** — `the persona is not on the strip: "│Chat │ Sally ● …"` |
| `the_tab_bar_shows_no_persona_while_the_conversation_is_the_active_tab` | **FAILED** — `the strip still exists: "│Chat │ …"` (it asserts `Main` is on the row) |
| `the_tab_bar_names_every_open_tab_and_marks_the_active_one` | **FAILED** — the pre-existing test, updated from `Chat` to `Main` |
| `a_subagent_row_shows_its_name_agent_type_and_status` (sidebar) | **FAILED** — the row still carried the descriptor |
| `a_rendered_subagent_row_shows_the_facts_and_not_the_prose` (sidebar) | **FAILED** — the rendered row still carried the descriptor |

Totals at the pre-P002 state: `tui_main_buffer` **65 passed, 5 failed**;
`cargo test -p codegg --lib tui::components::sidebar` **15 passed, 2 failed**.

Three negative controls passed on **both** sides, which is what makes the seven
failures above evidence rather than noise:
`the_tab_bar_keeps_its_labels_and_drops_the_persona_when_the_row_is_narrow`,
`the_tab_bar_keeps_its_content_on_its_own_row`, and
`no_tabs_means_no_strip_row_and_no_hints`.

### 4.3 Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib tui::app::state::subagent_tabs` | 38 passed, 0 failed |
| `cargo test -p codegg --lib tui::components::sidebar` | 17 passed, 0 failed |
| `cargo test -p codegg --lib tui::app::render` | 8 passed, 0 failed |
| `cargo test -p codegg --lib tui::` | 2585 passed, 0 failed |
| `cargo test -p codegg --test tui_main_buffer` | 70 passed, 0 failed |
| `cargo test -p codegg --test tui_render` | 111 passed, 0 failed |
| `cargo test -p codegg --test tui_side_panels` | 31 passed, 0 failed |
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, no warnings |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test` | pass |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py` | pass |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_no_hardcoded_selection_colours.py` | pass |
| `python3 scripts/check_first_party_skills.py` | pass |
| `scripts/verify.sh quick` | pass — "Quick verification passed." |

### 4.4 Not run

- `scripts/verify.sh full` — the nextest workspace sweep and the
  `--features server,plugins,lsp-test-support` pass were **not** run. The
  change is confined to the TUI and touches no protocol, storage, or server
  path.
- `cargo test -p codegg` in full (the whole crate, ~7 200 lib tests plus every
  integration binary) was **not** run; the TUI-filtered lib sweep and the four
  TUI integration binaries were.

**One pre-existing test is load-sensitive and was observed to fail under
concurrent load, unrelated to this change.**
`src/tui/commands/interactive_terminal.rs:3461`
(`a_hidden_terminal_surface_does_not_arm_the_loop`) waits for the terminal
create command in a single 25 ms drain, and the file's own doc comment at
`src/tui/commands/interactive_terminal.rs:3033` records that one drain "is not
enough". It failed twice while a 2 580-test sweep was running concurrently in
the same worktree, and passed 8/8 in isolation on an idle machine **both** at
the baseline and with this change applied. The final TUI-filtered lib sweep
(§4.3) was run on an idle machine and is green. It is recorded here rather
than left to be discovered as a surprise.

### 4.5 Wider sweep

`scripts/verify.sh quick` was run twice on the final tree state and passed both
times ("Quick verification passed."). It covers the agent schema, the
core/client/desktop boundaries, the sandbox, execution-ownership, TUI authority,
http-route-disposition, audit-coverage, queue-delivery-callsites,
queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, the
no-hardcoded-models guard, the provider wire/catalog/resilience guards, the
OpenAI endpoint guard, Eggwork target routing, and a workspace `cargo check`.

On the **merged** tree (after reconciling with `main` at `2a0bae08f`, merge
commit `fffd96426`) the root crate's library suite was also run in full:
`cargo test -p codegg --lib` — **7 236 passed, 6 failed, 4 ignored**, against
**7 211 passed, 3 failed, 4 ignored** on the `main` baseline. The three extra
failures are the load-sensitive `src/tui/commands/interactive_terminal.rs`
badge/PTY tests, a file neither this milestone nor upstream's Helix
workstream touches; they pass in isolation, single-threaded, on both trees.
P004's §4.5 carries the full per-test comparison. The PR's CI `verify` job —
the full workspace suite under nextest, on Linux runners — is **green**:
**15 674 tests run, 15 674 passed, 7 skipped**, 0 failed, with all six of the
locally-failing tests passing there.

## 5. Invariant review

- **The persona is the projection's value.** It is read from
  `SubagentRow::profile` through `SubagentRow::descriptor()` on the row the
  tab already carries. No name, pool, or lease is consulted in the TUI, and
  `check_tui_subagent_name_pool_isolation.py` is green.
- **A snapshot recorded before descriptors existed renders what it rendered
  before.** `descriptor()` returns `None` for a blank profile, and the tab
  carries `persona: None` — not `Some("")`. There is no placeholder and no
  ellipsis where nothing used to be.
- **A detached tab keeps the persona it last carried.** The persona is read
  from the tab's captured row, so it survives the subject leaving the
  projection, exactly as the tab body's own rule requires.
- **The row never over-paints.** The budget's geometry and the renderer's
  geometry are the same numbers, and a unit test walks widths 0–40 asserting
  the painted width never exceeds the row.

## 6. Failure and recovery review

No state, async, or cancellation semantics change. The milestone is pure
presentation, and the one behavioural change to an existing surface is the
side-panel row's content: it no longer spends its column on prose. Nothing
there was load-bearing — the row's identity, its focus target, and its hit
rect are unchanged.

## 7. Migration and compatibility review

No schema, protocol, or configuration change. The only API removal is
`MIN_SIDEBAR_DESCRIPTOR_CHARS` (§3.1 deviation 6), a `pub` constant in the TUI
whose referent no longer exists. `MAIN_TAB_LABEL`'s **value** changed
(`Chat` → `Main`); the constant's name and type are unchanged.

## 8. Security review

No new input, no new parsing, no new external call. The persona is rendered as
text, and `fit_persona` flattens control characters to spaces so a profile
carrying a newline cannot break the one-line row. No secret, path, or
identifier is added to any surface. The pool-isolation guard is green.

## 9. Documentation and operations

- `docs/tui.md` §"Subagent tabs" — rewritten: the strip's content, the status
  markers, the persona's home and its style, the stated keys, and the one
  width budget with its precedence.
- `architecture/tui.md` §"One row, five surfaces" — rewritten: the sidebar row
  is a scan surface, the persona lives on the strip, and the strip's row is one
  budget with a fixed precedence.
- `.opencode/skills/tui/SKILL.md` — two new bullets, so the skill and the
  architecture doc move together as `AGENTS.md` requires.
- `src/tui/input.rs:242` — the `⇧N` doc comment names `Main`.
- No user-facing operational change: no new key, no new command, no new
  configuration.

## 10. Unresolved findings

- **The persona is invisible on a narrow row.** This is the plan's precedence,
  not a defect, but it is worth stating plainly: the hints take 23 columns, so
  on an 80-column row the persona is dropped once about five subagent tabs are
  open. If the persona is found to matter more than the labels in practice, the
  precedence in §5 is the thing to revisit — and that is a product decision,
  not a bug fix.
- **The hint text is 23 columns wide** on a row that may be 40 columns wide.
  The plan's wording was used verbatim; shortening it is available if the row
  proves too narrow in practice.
- The load-sensitive terminal test described in §4.4 is **pre-existing** and
  was not fixed here; it is not in this milestone's blast radius.

## 11. Roadmap disposition

**P002 is closed.** P003's hard dependency — "P002 must be closed, not merely
started" — is satisfied by this record, and P003's product coupling is
satisfied because the persona now lives on the tab before P003 takes the
descriptor off the side-panel row.

P001 stays closed (`plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md`).
P004 (the two-pass name allocation) is unaffected and remains ready. The
addendum's status table and `plans/registry.md` are updated in §12.

## 12. Registry updates

- `plans/registry.md` — the P002 row moves to **closed**, with this closure
  path, the seven acceptance criteria's evidence, and the three deviations that
  a reader of the plan would otherwise trip over. The addendum's row records
  that P002 is closed and that P003 is unblocked.
- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md`
  — the §12 status table gives P002 its closure path and drops P003's
  "blocked on P002" note; §4.2 is marked as a **baseline record** so its
  citations stay pinned to `31a4128`.
- `plans/implementation/tui-subagent-presentation-and-name-allocation/002-tab-bar-identity-persona-and-hints.md`
  — status → `closed`, with an `Outcome:` paragraph, and §3 marked as a
  baseline record.
