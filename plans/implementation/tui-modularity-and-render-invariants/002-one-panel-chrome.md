# TUI Modularity and Render Invariants Milestone 002 — One panel chrome

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-002--one-panel-chrome`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the panel
  column presents the project state hierarchy; this milestone changes how the
  column is framed, not what it shows.

Applicable ADRs:

- None required. This is a local rendering change inside one module tree. It
  introduces no protocol, no storage, no authority and no compatibility
  surface, so it does not meet the ADR threshold.

Primary class: capability

## 1. Objective

One `PanelChrome` primitive that paints a panel column's border, title and
focus marker, adopted by all five bespoke panels and the hosted host, so that
"this panel is focused" is indicated the same way on every edge — and closes
invariant I3.

## 2. Why this milestone is ready

Blocked on M001 only, and that edge is a verification gate rather than a design
dependency: M002's exit condition is "I3 green", which needs the harness. The
chrome policy itself is fully specified below and needs nothing from M001.

Interface dependency on M003 is stable: `Section` (M003) reuses
`PanelChrome`'s focus-style resolution, and this plan fixes that resolution.

## 3. Current implementation evidence

At baseline `7642b9e38`, six surfaces express panel focus six ways. Every row
below was read from source, not inferred.

| Surface | Site | Outer border | Border on focus | Title on focus | Size guard |
|---|---|---|---|---|---|
| Chat | `src/tui/panels/chat.rs:279` | `Borders::ALL` | `primary` : `border` | `primary` : `muted` | `width < 12 \|\| height < 4` |
| Terminal | `src/tui/panels/terminal.rs:207` | `Borders::ALL` | `primary` : `border` | `primary` : `muted` | `width < 12 \|\| height < 4` |
| Session | `src/tui/panels/session.rs:35` → `src/tui/components/sidebar.rs:2214` | `Borders::ALL` | `primary` : `border` | **no `title_style` set** | none |
| Agents | `src/tui/panels/agents.rs:195` | `Borders::ALL` | **always `border`** | **always `primary` + `Modifier::BOLD`** | none |
| Help | `src/tui/panels/help.rs:82` → `src/tui/components/dialogs/help.rs:494` | **none** | n/a | title passed to the first inner box | none |
| Hosted ×18 | `src/tui/panels/hosted.rs:110` | `Borders::ALL` | **always `border`** | **no `title_style`** | none |

Three consequences follow, and each is a defect a user can see:

1. A focused Agents panel is indistinguishable from an unfocused one except by
   the `●` glyph, because neither the border nor the title colour changes.
2. A focused Session panel changes its border but not its title, while a
   focused Chat panel changes both.
3. A docked Help panel has no outer border at all, so its title — including the
   focus dot that `panels/help.rs:82` computes as `" Help ● "` — rides the
   first inner box's border instead of the panel edge.

`src/tui/panels/agents.rs:190` also hardcodes the literal `" AGENTS ● "`
rather than reading `SidePanelId::Agents.label()`, so the panel name is a
second source of truth for the identifier.

`tests/tui_render/tui_agents_panel.inc:77` asserts
`app.panel_host.is_focused()` — state, not appearance — which is why none of
the above is caught today.

## 4. Invariants that must not regress

- **No panel loses content.** Every panel paints the same rows and columns of
  content after adoption, except where this plan explicitly records a
  geometry change (Help, §5).
- **No panel loses its title or its focus marker.** Both remain, on the outer
  border, for every panel.
- **No panel gains a key.** Panel key ownership is unchanged; a stray `j`
  still never reaches the composer.
- **I1 and I2 stay green.** Adoption must not paint outside the area or panic
  at 1×1.
- **The render-cost shape guard is unchanged.** One chrome producer is a
  reduction in work, not a second producer.
- **The comment ratchet holds.** No new file exceeds
  `unlisted_max_comment_lines = 20` without a manifest entry.

## 5. Scope

### In scope

- `PanelChrome` plus a shared "does this area have chrome" predicate.
- Adoption by Chat, Terminal, Session, Agents, Help and the hosted host.
- I3b: focused and unfocused renders differ in **border style or title
  style**, uniformly, wherever chrome is drawn.
- The corpus diff for every panel, reviewed and recorded.

### Explicitly out of scope

- Any change to panel content, key handling, docking gestures, or the panel
  manager.
- Spacing, glyph and casing tokens (§3 of the roadmap).
- Dialog chrome — that is M003.
- Making Help's title row disappear. Help keeps a title; it gains an outer
  border to put it on.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

- A new module in `src/tui/panels/` exposing `PanelChrome` and the area
  predicate. `PanelChrome` owns exactly four decisions: whether chrome is
  drawn, the border style, the title string and the title style. It returns the
  inner `Rect` so callers do not re-derive the inset.
- **The focus policy, fixed here so M003 can reuse it:**
  - border style — `theme.primary` when focused, `theme.border` otherwise;
  - title style — `theme.primary` when focused, `theme.muted` otherwise;
  - the focus marker is a trailing `●` inside the title, always present when
    chrome is drawn;
  - `Modifier::BOLD` is **not** part of the focus signal. Agents currently
    applies it unconditionally, which carries no information.
- **The chrome predicate, reusing the guard Chat and Terminal already apply:**
  chrome is drawn when `area.width >= 12 && area.height >= 4`. Below that the
  panel paints borderless, exactly as Chat and Terminal already do, and I3b is
  not asserted.
- Agents reads `SidePanelId::Agents.label()` for its title instead of the
  literal.

### Security and authorization

None. No key, command or scope changes.

### Documentation and static guards

- `architecture/tui.md` — the panel section gains the focus policy and the
  chrome predicate, because both are now contracts a new panel must follow.
- No new guard. I3b in the M001 harness is the enforcement.

## 7. Ordered work packages

### Work package A — The primitive

Intent: one producer of panel chrome.

Required changes: `PanelChrome { title, focused, theme }` with `draw(area,
buf) -> Rect` returning the inner area, and `has_chrome(area) -> bool`.
Parameterise the title so Help can pass its own; do not hardcode a panel name
inside it.

Acceptance evidence: a unit test asserts the four decisions for the focused and
unfocused cases, and that `draw` returns an inset of exactly one cell on each
drawn edge.

### Work package B — Chat and Terminal

Intent: adopt where the policy already matches, so the primitive is proved
against two surfaces that must not change.

Required changes: replace the hand-rolled `Block` with `PanelChrome`.

Acceptance evidence: the corpus for both panels is **byte-identical** at
80×24 and 120×40, focused and unfocused. This is the control case: if it
moves, the primitive is wrong, not the panel.

### Work package C — Session

Intent: adopt where the border already matches and the title does not.

Required changes: `SidebarWidget::render_with_title` takes its border and
title style from `PanelChrome` rather than building a `Block` itself.

Acceptance evidence: corpus diff shows exactly the title colour changing on
focus, and nothing else. Recorded in the milestone's diff note.

### Work package D — Agents

Intent: adopt where focus is currently invisible.

Required changes: `PanelChrome`; title from `SidePanelId::Agents.label()`.

Acceptance evidence: corpus diff shows the border colour changing on focus and
`BOLD` no longer applied; no content row moves. A focused Agents panel and an
unfocused one differ in border and title style.

### Work package E — Help

Intent: give the panel an edge for its title to sit on.

Required changes: `HelpPanel::render` draws `PanelChrome` and passes the inner
area to `HelpDialog::render_into`. The dialog keeps its four-section internal
layout; it stops being responsible for the panel's outer chrome.

Acceptance evidence: corpus diff shows the outer border appearing, the title
including the focus dot on that border, and the inner content inset by one
cell on each side. The content must remain fully readable at 20×6 — if it does
not, that is a stop condition (§14).

### Work package F — Hosted host

Intent: make the 18 hosted panels indicate focus the same way as the five
bespoke ones.

Required changes: `render_hosted` builds its chrome through `PanelChrome`
instead of an inline `Block`, so the title string no longer carries the focus
decision.

Acceptance evidence: corpus diff is reviewed **per hosted panel**, not
summarily. This is the widest blast radius in the milestone: 18 surfaces move
at once. A single combined diff is not acceptable evidence.

### Work package G — I3b

Intent: make the uniformity an assertion rather than a review outcome.

Required changes: extend the M001 harness so that, for every surface where
chrome is drawn, the focused/unfocused difference is a border-style or
title-style difference.

Acceptance evidence: the assertion fails against the pre-adoption Agents
fixture (border and title both unchanged) and passes after adoption — proving
it detects exactly the defect this milestone fixes.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Rendering is synchronous and stateless with respect to this
change. The only failure mode is a paint that reads a theme field it should not
have, which the corpus catches.

## 9. Compatibility and migration

No compatibility surface. Panel placement, docking, focus and key ownership are
unchanged, so a user's gestures behave identically. The visible change is
confined to border and title styling plus Help's new outer border.

## 10. Required tests

### Focused unit tests

- `PanelChrome` resolves border and title styles for focused and unfocused.
- `has_chrome` is false below 12×4 and true at and above it.
- `draw` returns an inner rect inset by one on each drawn edge.

### Integration tests

- Every panel renders at 20×6, 40×12, 80×24 and 120×40, focused and unfocused.
- I3b holds for every panel where chrome is drawn.
- I1 and I2 remain green for every panel.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- I3b fails against a fixture whose focus changes nothing — the pre-adoption
  Agents behaviour — so the assertion is not vacuous.
- A panel rendered below the chrome threshold paints no border and does not
  panic.

### Migration and compatibility tests

- A test asserts the pre-adoption and post-adoption content rows are identical
  for Chat and Terminal (the control case), so "no content moved" is a checked
  property rather than a reviewer's impression.

## 11. Required verification commands

```bash
# the invariant harness, including I3b
cargo test --test tui_render -- tui_render_invariants

# panel surfaces specifically
cargo test --test tui_render -- tui_agents_panel
cargo test --test tui_render -- tui_chat_surfaces
cargo test --test tui_render -- tui_info_dialog_panel_chrome
cargo test --test tui_render -- tui_panel_manager_reachability

# the render-cost shape guard
python3 scripts/check_tui_render_cost.py

# the comment ratchet
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings

# the routine gate
scripts/verify.sh quick
```

## 12. Documentation updates

- `architecture/tui.md` — the focus policy and the chrome predicate, stated as
  the contract a new panel follows.
- `architecture/theme.md` — if the `primary`/`border` and
  `primary`/`muted` pairings are described anywhere, they now have one
  authoritative definition.
- The milestone's corpus diff note, listing each panel and the exact cells
  that changed.

## 13. Acceptance criteria

1. Every panel obtains its border, title and focus marker from `PanelChrome`.
2. No panel hardcodes its own name in a title.
3. I3b passes for every panel where chrome is drawn, and is proved non-vacuous.
4. Chat and Terminal corpora are byte-identical (control case).
5. Every other panel's corpus diff is recorded per panel and reviewed.
6. Help's content remains fully readable at 20×6.
7. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.
8. No panel gained or lost a key.

## 14. Stop conditions

Report rather than improvise when:

- adopting `PanelChrome` moves Chat or Terminal at all — the primitive is
  wrong and the control case exists to catch it;
- Help cannot remain usable at 20×6 with an outer border — report the
  measurements and propose a smaller chrome threshold rather than deleting the
  border or the title;
- a hosted panel's corpus diff shows a content change rather than a chrome
  change — that is a regression in the host, not an intended effect;
- the focus policy in §6 conflicts with a theme whose `primary` and `border`
  are the same colour, making focus invisible again. That is an I5/I3
  interaction: report it and let M007 own the token question. Do not invent a
  fallback colour here.

## 15. Closure evidence required

- The primitive and its unit tests.
- I3b output, plus the pre-adoption failure that proves it non-vacuous.
- The per-panel corpus diffs, Chat and Terminal marked byte-identical.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/002-status.md`.

## 16. Handoff notes

The control case is the point of this milestone. Chat and Terminal already
implement the target policy; if adoption changes their output, stop. Everything
else in the milestone is measured against that proof.

The hosted host is where this can go wrong quietly. Eighteen panels share one
paint path, and a single summary diff would hide a content shift in one of
them. Review per panel.

Finally: this milestone makes Agents look different, and that is intended. A
focused Agents panel gaining a border colour is an improvement, and the roadmap
records it as a deliberate change rather than a regression. Do not "preserve"
the old look to keep a diff small.
