# TUI Subagent Presentation and Name Allocation — Milestone P002 — The Tab Bar Is the Identity Surface

Status: closed — closure `plans/closure/tui-subagent-presentation-and-name-allocation/002-status.md`

Outcome: the strip is now the identity surface it was meant to be, and it says
so on the one row it already occupies. The main entry reads `Main` and wears
`▣` — a square, from a different shape family than the three liveness glyphs,
because the main conversation is not a run and must not be mistaken for one.
The **active** tab's persona is on the row, in italics, directly after the entry
it describes; the row states `⇧P/⇧N switch · ⇧X close` on the right; and the
side panel's subagent row is now name, agent type, status, with no prose.

All four share **one width budget**, computed once in `plan_strip` in a fixed
precedence: the hints are reserved first and in full, the labels keep their
text, and the persona takes what is left and is dropped rather than squeezed
below a legible floor. The strip's width arithmetic did not count the status
glyphs before this milestone — it does now, because a marker that is painted
but not measured paints past the width the budget promised, which is how it
would have ended up under the hints.

**The plan's own §5 precedence list and §8 degradation order contradict each
other**; §10's required test ("labels are preferred over the persona") was
followed, and the consequence is recorded rather than buried: on an 80-column
row the persona is visible up to about four open subagent tabs and dropped
beyond that. The closure record's §3.1 holds this and the other three
deviations, and its §4.2 holds the evidence that the new tests fail against the
pre-P002 code.

Repository baseline: `31a412844fbbf36055b29245075121035eefc385`

Source roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P002)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles` — §4.4 *Frontends render projections*, §4.6 *Progressive disclosure*.
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`

Applicable ADRs: none.

Primary class: **capability** (K4, K5) + **polish** (P1)

## 1. Objective

Make the one-row subagent tab strip the canonical identity surface for the thing it describes, and state the keys that operate it:

- the main-conversation entry reads **`Main`**, not `Chat`, and carries a **glyph** so it reads as a tab rather than as plain text;
- the **active** tab's **persona** appears on the strip, in **italics**, truncated to what actually fits;
- the strip states **`⇧P` / `⇧N` / `⇧X`** — on the row it already occupies, with no extra line anywhere;
- the side panel's subagent row drops the prose descriptor and shows **name, agent type, status**.

All four share **one row and therefore one width budget**. They are one milestone because splitting them would mean designing the budget twice.

## 2. Why this milestone is ready

- **Hard dependencies:** none.
- **Soft dependency:** P001 (`001-tab-surface-ownership.md`) also edits `render_subagent_tab_strip`. P002 is not blocked on P001's correctness, but the two should be serialized so they do not conflict in one function. If P001 has not landed, re-read the strip function first — P001 may have restructured it.
- P002 changes no projection, protocol, or storage path.

## 3. Current implementation evidence

> This section is the **baseline record** at `31a4128`, kept as the plan wrote
> it. Its line citations are pinned to that commit and are deliberately **not**
> refreshed: they describe the state the plan was written against, and the
> change this milestone made is the difference between them and the tree. The
> closure record's §3 carries the post-change anchors.

### 3.1 The label and the missing glyph

`MAIN_TAB_LABEL = "Chat"` (`src/tui/app/state/subagent_tabs.rs:58`), consumed by `bar_tabs()`:

```rust
// subagent_tabs.rs:392-411
let mut out = vec![SubagentBarTab {
    label: MAIN_TAB_LABEL.to_string(),
    main: true,
    active: self.active_tab_id.is_none(),
    glyph: "",            // <- the main entry has no glyph
    ellipsis: false,
}];
for tab in self.ordered() {
    out.push(SubagentBarTab {
        label: tab.label().to_string(),
        main: false,
        active: self.active_tab_id.as_ref() == Some(&tab.tab_id),
        glyph: tab.glyph(),      // "●" live, "○" finished, "✕" detached
        ellipsis: false,
    });
}
```

`SubagentTabState::glyph()` (`subagent_tabs.rs:165-170`) returns a **liveness** glyph. The main entry's empty glyph therefore reads as plain text, not as a tab.

`docs/tui.md:732` ("The cycle runs `Chat` first") and the doc comment on `InputAction::NextSubagentTab` (`src/tui/input.rs:242`, "Cycle order runs `Chat` → subagent tabs") both name the old label.

### 3.2 The row exists; no other row may be used

The strip row is reserved by `subagent_tab_strip_height` (`src/tui/app/mod.rs:15113-15115`) and is **one** row, present only while tabs are open.

The composer hint line must not be reused. `hint_rows = u16::from(self.composer_hint_text().is_some())` (`src/tui/app/render.rs:175`) — the hint row is reserved from the prompt allocation **only when it is present**. Appending tab hints there would therefore reserve a **new** row and reflow the transcript on every submit and completion. The strip row is already reserved and is the only correct home.

### 3.3 The keys exist and are undiscoverable

| Action | Binding | Site |
|---|---|---|
| `InputAction::NextSubagentTab` | `Shift+N` | `src/tui/input.rs:242-245`, mapped `input.rs:795-797` |
| `InputAction::PreviousSubagentTab` | `Shift+P` | `src/tui/input.rs:246-248`, mapped `input.rs:795-797` |
| `InputAction::CloseSubagentTab` | `Shift+X` | `src/tui/input.rs:249-252`, mapped `input.rs:795-797` |

`CloseSubagentTab` is handled at `src/tui/app/mod.rs:15173-15179` and is named in the tab-capacity toast (`app/mod.rs:15141`). **`Shift+W` is `OpenWorkspaceDashboard`** (`input.rs:1401-1407`) and must not be rebound — the parent convergence addendum recorded this as withdrawn decision **D-C3**.

Hint formatting convention: `src/tui/panels/session.rs:53-57` renders `"SESSIONS · jk move · ↵ open · space fold · hl collapse/expand · ⇧S panel"` — `·`-separated fragments, `⇧` for Shift.

### 3.4 The persona

`SubagentRow::profile` is the pool's character description, carried on the spawn event and copied verbatim through the projection; `SubagentRow::descriptor()` (`src/tui/app/state/subagents.rs:104-113`) returns it, and `SubagentRow::topic()` (`:122-127`) prefers it over the task description for one-line surfaces.

The persona is **already reachable from the tab**: `SubagentTabState` holds the whole row (`subagent_tabs.rs:123-133`, `pub row: SubagentRow`), and the tab body renders it under `Agent` (`src/tui/app/render.rs:1567-1591`).

Its size is the constraint: the pool's longest `profile` is 223 B (the C005 closure record's measured maximum), i.e. one to three sentences.

### 3.5 The side panel's row spends its column on prose

`src/tui/components/sidebar.rs:1396-1427`:

```rust
let (icon, style) = self.subagent_glyph(&row.status);
let target = SidebarFocusTarget::Subagent(row.identity_key().to_string());
let agent = clean_inline_text(&row.agent, 16);
let status = clean_inline_text(&row.status, 12);
let name = clean_inline_text(&row.name, 24);
let tail = format!(" ({agent}) {status}");
// ... the descriptor takes whatever the identity above did not use ...
let used = 4 + name.chars().count() + tail.chars().count();
let room = (area_width as usize).saturating_sub(used + 3);
```

When `room >= MIN_SIDEBAR_DESCRIPTOR_CHARS` (12, `sidebar.rs:205`) the row appends `" — {profile}"`. Two tests pin this and must change with it:

- `a_subagent_row_shows_its_descriptor_when_the_column_has_room` (`sidebar.rs:2296`)
- `a_subagent_row_drops_the_descriptor_when_the_column_is_too_narrow` (`sidebar.rs:2319`)

### 3.6 The width arithmetic

`SubagentBarTab::label_width()` (`subagent_tabs.rs:437-441`) measures **display** width via `unicode_width`, not character count — correct, and to be preserved. `bar_width()` (`:443-448`) sums label widths plus the separator width. `fit_bar_tabs()` (`:514-569`) keeps the main entry plus a contiguous run of the newest tabs, falling back to the active entry alone if it does not fit, and finally truncates labels in place.

**Any new content on the strip must be counted in this arithmetic**, not painted on top of it.

## 4. Invariants that must not regress

- **I1 — Projection-derived rows.** The persona shown on the tab is the value the projection carried (`SubagentRow::profile`). The TUI has **no** curated pool and must not grow one: `scripts/check_tui_subagent_name_pool_isolation.py` fails if any module under `src/tui/` references the pool, and it must stay green.
- **I2/I3** — from P001: the tab owns its pixels; the strip row is opaque. If P001 has not landed, do not duplicate its background work here — land P001 first or coordinate.
- **I6 — Tab ordering is not an execution boundary.** The strip's hints describe navigation and closing only.
- **The working strip row 0** stays reserved and stays the main buffer's only status surface. The tab strip is a different row and must not move.
- **Display width, not character count**, for every measurement on this row.
- `MAX_OPEN_SUBAGENT_TABS = 16` (`subagent_tabs.rs:55`) and the capacity refusal are unchanged.

## 5. Scope

### In scope

- `MAIN_TAB_LABEL` and the main entry's glyph.
- The persona on the active tab.
- The key hints on the strip row.
- The width budget that covers all three.
- The side panel's subagent row shape.
- The `Chat` wording in `docs/tui.md` and `src/tui/input.rs`.

### Explicitly out of scope

- The viewport and the tab body (P001).
- The side panel's **navigation** (P003).
- The persona on any other surface. The `Ctrl+T` overlay, the `/sessions` dialog, the working strip, and the tab body all keep it.
- A persona on **every** tab. One row cannot carry one per tab; only the active tab's persona is shown.
- Rebinding `Shift+W`, adding a close key, or changing any binding. The keys exist.
- The persona's content, its byte bound, or its projection path.
- A new row anywhere. The strip's row is the only row this milestone may use.

## 6. Required production changes

### Frontend — the strip's content

1. **Rename the main entry.** `MAIN_TAB_LABEL` becomes `"Main"`. The constant's doc comment ("Label for the always-present main-conversation entry in the tab bar") stays accurate.

2. **Give the main entry a glyph.** It must be **visually distinct from `●`, `○`, and `✕`**, because those three encode a *run's liveness* and the main entry is not a run. The implementer chooses the glyph and records the choice in the closure record; the constraint (distinct from the three liveness glyphs, legible at one column) is what is binding.

3. **Show the active tab's persona, in italics.** The persona comes from the active tab's `SubagentRow::profile` — the projection's value, never a frontend lookup. It is rendered in italics (`Modifier::ITALIC`).

   *Why only the active tab:* a persona is up to 223 B; a strip is 80–200 columns and must also carry every label plus the hints. One persona per tab is not a budget question, it is arithmetic.

   *Where it lives:* `SubagentBarTab` (`subagent_tabs.rs:415-426`) is the struct the strip renders from, and it currently carries `label`, `main`, `active`, `glyph`, `ellipsis`. It gains whatever field the strip needs to carry the persona (for example `persona: Option<String>`), populated in `bar_tabs()` **for the active entry only**. `bar_tabs()` builds entries from `self.ordered()`, and each `SubagentTabState` holds its `row`, so the value is available without a new lookup.

   *Truncation:* the persona is truncated to the width that remains **after** the labels and the hints are paid for — using the same display-width arithmetic as `label_width` / `bar_width` / `truncate_to_width`. It is never allowed to push a label off the row.

4. **Show the key hints on the strip row.** `⇧P`, `⇧N`, `⇧X`, formatted in the house style (`·`-separated, `⇧` for Shift — see `src/tui/panels/session.rs:53-57`). They appear **only while tabs are open**, because the strip row itself only exists then (`subagent_tab_strip_height`). Suggested shape: `"⇧P/⇧N switch · ⇧X close"`; the implementer may word it, but all three keys must be present and recognisable.

   The hints sit on the **right** of the row and the tabs on the left, with the two regions separated by the existing separator style. The hints must not be painted over a tab label.

### Frontend — one width budget

5. **Compute the budget once.** The strip's width is divided between three consumers, in this **precedence** order:

   1. the **hints** — reserved first and in full; they are short, fixed, and are the discoverability fix this milestone exists to deliver;
   2. the **labels** — every visible tab keeps its label; `fit_bar_tabs` already drops the oldest tabs and leaves an ellipsis, and it must keep doing so rather than truncating a label to make room;
   3. the **persona** — what remains after 1 and 2, truncated to fit; if what remains is too small to be meaningful, the persona is **dropped**, not rendered as a two-character stub.

   A stub persona is worse than no persona: the codebase already rejected that trade for the side-panel column ("a descriptor squeezed to three characters reads as a rendering fault rather than as prose", `sidebar.rs:1405-1407`). Apply the same rule here.

   The reserved hint width must be subtracted **before** `fit_bar_tabs` is called, so the tab-fitting logic works on the width the tabs actually have.

### Frontend — the side panel's row

6. **Drop the descriptor from the subagent row.** The row becomes `icon name (agent) status` and nothing else. `MIN_SIDEBAR_DESCRIPTOR_CHARS` and the `room` computation it feeds are removed with it.

   The two pinned tests (`sidebar.rs:2296`, `:2319`) are **updated to the new behavior**, not deleted: the first becomes "a subagent row shows name, agent type and status", and the second becomes "a subagent row shows the same facts in a narrow column" (i.e. the descriptor no longer changes with the column width).

   The row keeps its identity target, its glyph, and its selection styling. Only the prose goes.

   **This is the persona's move, not the persona's loss.** The persona remains on the tab strip (new), the working strip, the `Ctrl+T` overlay, the `/sessions` dialog, and the tab body.

### Documentation and static guards

- `src/tui/input.rs:242` — the "Cycle order runs `Chat` → subagent tabs" doc comment becomes `Main`.
- `docs/tui.md:732` — "The cycle runs `Chat` first" becomes `Main`.
- `docs/tui.md` §"Subagent tabs" — document the strip hints and the persona placement.
- `architecture/tui.md` §"One row, five surfaces, and a deliberate trade" (around `:759-770`) — reconcile with the new placement rule: the tab strip is the identity surface and carries the persona for the active tab; the side-panel row does not.
- `scripts/check_tui_subagent_name_pool_isolation.py` must stay green. Run it.

## 7. Ordered work packages

### Work package A — The budget, before the content

Intent: one width arithmetic that three consumers share, established before any of them is drawn.

Required changes: extend the strip's width computation to reserve the hint width, to account for the persona, and to hand the remainder to `fit_bar_tabs`. Extract the computation into a pure function that can be unit-tested without a terminal.

Acceptance evidence: a unit test asserting that a strip too narrow for labels + hints + persona drops the **persona** first and keeps every label; and that the hints are never truncated.

### Work package B — The strip's content

Intent: `Main`, a glyph, the italic persona, and the three key hints, all on the existing row.

Required changes: `MAIN_TAB_LABEL`; the main entry's glyph; the persona field on `SubagentBarTab`; the italic span; the right-aligned hints.

Acceptance evidence: a render test asserting the strip shows `Main`, the three key hints, and the persona in italics when a subagent tab is active; and a render test asserting the persona is absent when the **main** entry is the active one (the main conversation has no persona).

### Work package C — The side-panel row

Intent: the row carries three scannable facts and no prose.

Required changes: remove the descriptor append and the `room`/`MIN_SIDEBAR_DESCRIPTOR_CHARS` machinery; update the two pinned tests.

Acceptance evidence: the two updated tests; a render test asserting the row shows the name, the agent type and the status and **not** the persona's first words.

### Work package D — Documentation

Intent: every place that names the old label or the old placement is corrected.

## 8. Failure, cancellation, restart, and contention semantics

- No state, no async, no cancellation semantics change. This milestone is pure presentation.
- The persona must come from the **active tab's captured row**, so a tab whose run has left the projection still shows the persona it last carried — the same rule the tab body already follows.
- A tab set with many tabs and a long persona must degrade by dropping tabs (existing `fit_bar_tabs` behaviour) and then the persona, never by overflowing the row.

## 9. Compatibility and the persona path

No schema, protocol, or configuration change. A snapshot recorded before descriptors existed carries no `profile`; the tab then shows no persona, which is the same "render what this surface rendered before" fallback every other surface already uses. Do not synthesize a placeholder persona.

## 10. Required tests

### Focused unit tests

- `bar_tabs()`'s first entry has label `Main` and a non-empty glyph that is not `●`/`○`/`✕`. (The existing test asserting `entries[0].label == MAIN_TAB_LABEL` at `subagent_tabs.rs:848` continues to hold and needs no change if it references the constant — confirm.)
- The active entry carries the persona; a non-active entry does not.
- A tab with no persona (pre-descriptor snapshot) yields `None`, not `Some("")`.
- The width budget: hints are reserved first; labels are preferred over the persona; the persona is dropped when the remainder is below a legible threshold.
- `fit_bar_tabs` receives the reduced width (the hint reservation is applied before the call).

### Integration / render tests

- The strip renders `Main`, `⇧P`, `⇧N`, `⇧X` when a tab is open.
- The persona renders in italics for the active subagent tab, and not for the main entry.
- The strip does not exceed its rect width at narrow widths (no panic, no overflow into the next row).
- The side-panel subagent row shows name, agent type and status, and not the persona.

### Negative tests

- With **no** tabs open, the strip row does not exist and the hints are absent.
- With a persona too long for the remaining width, the strip still renders every label and drops the persona.

### Security and negative tests

- A **negative guard test**: assert that no module under `src/tui/` references `SUBAGENT_NAMES` or the pool module (the guard's own `--self-test` covers this; run it).

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg --lib tui::app::state::subagent_tabs
cargo test -p codegg --lib tui::components::sidebar
cargo test -p codegg --lib tui::app::render

# static guards
python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test
python3 scripts/check_tui_subagent_name_pool_isolation.py
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_no_hardcoded_selection_colours.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh quick
```

Do not claim any command that was not actually run.

## 12. Documentation updates

- `src/tui/input.rs:242` — `Chat` → `Main`.
- `docs/tui.md:732` — `Chat` → `Main`.
- `docs/tui.md` §"Subagent tabs" — the strip hints, the persona placement, and the row's new content.
- `architecture/tui.md` — the "one row, five surfaces" paragraph and the persona-placement rule.

## 13. Acceptance criteria

1. The strip's main entry reads `Main` and carries a glyph distinct from `●`/`○`/`✕`.
2. With a subagent tab active, that tab's persona is visible on the strip, in italics, truncated to the remaining width or dropped — never overflowing and never a two-character stub.
3. With the main entry active, no persona is shown.
4. `⇧P`, `⇧N`, and `⇧X` are visible on the strip row whenever a tab is open, and absent when none is.
5. **No new row is introduced anywhere**; the transcript does not reflow when the strip's content changes.
6. The side-panel subagent row shows the name, the agent type, and the status, and no prose descriptor.
7. `scripts/check_tui_subagent_name_pool_isolation.py` is green — the persona is the projection's value.

## 14. Stop conditions

Stop and report rather than improvise when:

- Showing the persona on every tab seems required — it does not fit, and inventing a second row violates the "no extra line" requirement; report the conflict instead.
- The width budget cannot be made to fit all three consumers — report which one must lose, do not silently drop the hints.
- The persona would have to be resolved from the pool in the TUI — that is forbidden (I1); report it.
- P001's opaque-background work is not yet landed and the two would conflict in `render_subagent_tab_strip`.
- The change appears to require a new protocol field or a projection change.

## 15. Closure evidence required

- The implementation SHA.
- The requirement-to-evidence matrix over the seven acceptance criteria.
- The exact commands run and their results, including `scripts/verify.sh quick` and the pool-isolation guard with its `--self-test`.
- The chosen main-entry glyph and the reasoning.
- An explicit statement of what was **not** run.
- The documentation diff.

## 16. Handoff notes

- P002 and P003 are coupled by a **product** decision, not by code: P002 puts the persona on the tab, P003 takes it off the side-panel row. Do not remove the side-panel descriptor in this milestone, and do not land P003 before this milestone closes.
- `src/tui/components/sidebar.rs` is touched by this milestone **only** for the row's rendered content. Its focus model belongs to P003 — do not change `focus_targets` here.
- The strip is one row. If you find yourself wanting a second one, that is a scope question for the maintainer, not an implementation choice.
