# TUI Modularity and Render Invariants Milestone 009 — List cursor and scroll follow

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-009--list-cursor-and-scroll-follow`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — selection
  and navigation present the state hierarchy; this milestone changes who owns
  the cursor arithmetic, not what is selected.

Applicable ADRs:

- None required. Internal state-type consolidation inside `src/tui/components/`
  and `src/tui/app/state/`. No protocol, storage or authority surface.

Primary class: infrastructure

## 1. Objective

One `ListCursor` owning a clamped index and its scroll-follow, adopted where
the semantics genuinely match, so sixteen copies of one rule become one — and
so a new list surface has an obvious thing to reuse.

This milestone delivers **no user-visible capability**. Its acceptance evidence
is that the corpus did not move and that every converted surface kept the
policy it had.

## 2. Why this milestone is ready

Blocked on M001 (hard, verification gate). Independent of every other milestone:
list state is not chrome, not hints, not frame geometry and not command routing.

The primitive it generalises already exists and is in use:
`CenteredScroll` (`src/tui/components/scroll.rs`) is adopted by 12 files, so the
repository has already accepted the idea of a shared scroll rule. What is
missing is the *index* half, which is why 16 files each wrote their own.

## 3. Current implementation evidence

At baseline `7642b9e38`:

- `select_up` and `select_down` are each defined in **16 files**. The
  implementations are not one function wearing sixteen names, and the
  differences matter:
  - **Identical clamped decrement** — `keybind.rs`, `completion_overlay.rs`
    and `project_picker.rs` all implement `if selected > 0 { selected -= 1 }`
    with a `saturating_sub(1)` bound on the way down.
  - **Clamp plus scroll-follow** — `agent.rs` additionally calls
    `CenteredScroll::clamp` when the cursor leaves the visible window.
  - **A different function entirely** — `mcp.rs` dispatches on `browse_mode`
    and `action_mode` to three different selected fields. It is not the same
    rule and deduplicating it naively changes behaviour.
- **29 dialogs declare their own scroll-offset field** while only 12 use
  `CenteredScroll`. So roughly 17 hand-roll scrolling as well as selection.
- `CenteredScroll::clamp(cursor, total, visible)` centres the cursor in the
  window and clamps at both ends; it does not own the index and has no notion of
  wrap.
- `project_picker.rs` takes `visible_count` as a parameter, so its cursor
  already knows the window size — the shape `ListCursor` needs.
- `KeyCode::Char('j')` appears in 34 dialog files, so the *key* mapping is also
  duplicated, but that is a dispatch concern and not this milestone's.

## 4. Invariants that must not regress

- **Every converted surface keeps its exact policy.** Clamp stays clamp; a
  surface that wraps keeps wrapping; a surface with a page step keeps it. The
  plan requires a test per converted surface naming its policy.
- **`McpDialog` behaviour is unchanged**, including its `browse_mode` and
  `action_mode` dispatch. It may use the index step; it may not be collapsed
  into a single cursor.
- **The corpus is byte-identical.** Selection highlighting, scroll position and
  list contents all render identically.
- **No surface gains or loses a key.** Navigation keys are not rebound.
- **I1–I6 stay green.**
- **The render-cost shape guard is unchanged.**
- **The comment ratchet holds.**

## 5. Scope

### In scope

- `ListCursor` with `index` and `offset`, and operations `up`, `down`,
  `page_up`, `page_down`, `home`, `end`, each taking the item count and the
  visible height.
- A `ScrollPolicy` distinguishing clamp from wrap, defaulting to clamp.
- Adoption by the surfaces whose semantics match: the identical-clamp group
  first, then the clamp-plus-scroll-follow group.
- Tests per converted surface naming its policy.

### Explicitly out of scope

- `McpDialog`'s mode dispatch. It keeps its own selection fields and uses the
  index step only if that is a genuine simplification.
- Surfaces whose scroll policy is genuinely different, such as a
  cursor-independent scroll or a horizontal scroll. They are recorded as not
  adopted, with the reason.
- Key mapping. `j`/`k`/arrow duplication is a dispatch concern; M010 is about
  command routing, not list keys, so this stays where it is.
- Any change to `CenteredScroll`'s centring rule.

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

- `ListCursor` in `src/tui/components/scroll.rs` alongside `CenteredScroll`,
  composing it rather than replacing it: `ListCursor` owns the index and calls
  `CenteredScroll::clamp` for the follow behaviour, so the existing centring
  rule is reused rather than reimplemented.
- `ScrollPolicy` as an explicit parameter, so a surface's choice is visible at
  its call site instead of implicit in its own arithmetic.

### Security and authorization

None.

### Documentation and static guards

- `architecture/tui.md` — the list-state contract: one cursor type, an explicit
  policy, and the statement that a surface with a different rule keeps its own
  and says why.

## 7. Ordered work packages

### Work package A — The type

Intent: one owner of index plus offset.

Required changes: `ListCursor` with the six operations, composed over
`CenteredScroll`, plus `ScrollPolicy`.

Acceptance evidence: unit tests for each operation at the boundaries — empty
list, single item, cursor at 0, cursor at the last item, visible height larger
than the list, visible height zero.

### Work package B — The identical-clamp group

Intent: convert the surfaces whose behaviour is already the same, so the type is
proved against a control case.

Required changes: `keybind.rs`, `completion_overlay.rs` and `project_picker.rs`
adopt `ListCursor` with `ScrollPolicy::Clamp`.

Acceptance evidence: the corpus for those three surfaces is **byte-identical**.
If it moves, the type is wrong, not the surface.

### Work package C — The scroll-follow group

Intent: convert the surfaces that also move their window.

Required changes: `agent.rs` and the other clamp-plus-follow surfaces adopt the
cursor, with their existing visible-height source.

Acceptance evidence: corpus byte-identical, and a test asserts that moving the
cursor past the window's edge scrolls by the same amount as before.

### Work package D — Non-adoption record

Intent: make "we looked and decided not to" visible.

Required changes: list every surface that keeps its own arithmetic and the
reason. `McpDialog` is the named example.

Acceptance evidence: the list is in the closure record and every entry names a
concrete difference, not a preference.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable. Cursor state is synchronous and per-surface. The one hazard is a
surface whose item count changes while its cursor is set — a filter narrowing
under a cursor. `ListCursor` must clamp on every read rather than trusting its
stored index, and a test asserts that a shrinking list clamps the cursor rather
than leaving it out of range.

## 9. Compatibility and migration

No compatibility surface. Selection and scrolling behave identically. A surface
that is not converted keeps its own implementation and is listed.

## 10. Required tests

### Focused unit tests

- Each `ListCursor` operation at the six boundaries named above.
- `ScrollPolicy::Wrap` wraps and `ScrollPolicy::Clamp` does not.
- A shrinking item count clamps a stale cursor.

### Integration tests

- Corpus byte-identical for every converted surface.
- A test per converted surface naming its policy and asserting the boundary
  behaviour, so a later change that swaps clamp for wrap fails.
- I1–I6 remain green.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- A cursor set beyond the item count renders a valid selection rather than
  panicking or highlighting nothing.

### Migration and compatibility tests

- The three control-case surfaces are byte-identical, which is the proof the
  conversion was behaviour-preserving.

## 11. Required verification commands

```bash
# the type
cargo test --lib -- tui::components::scroll

# converted surfaces
cargo test --test tui_render -- file_picker_surface_corrective
cargo test --test tui_render -- tui_agents_panel
cargo test --test tui_render -- tui_composer_claims
cargo test --test tui_render -- invariants

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

- `architecture/tui.md` — the list-state contract.
- The closure record's non-adoption list.

## 13. Acceptance criteria

1. `ListCursor` exists, composes `CenteredScroll`, and owns an explicit policy.
2. The three control-case surfaces are byte-identical.
3. Every converted surface has a test naming its policy.
4. `McpDialog` behaviour is unchanged and its test passes.
5. Every non-adopting surface is listed with a concrete difference.
6. A stale cursor clamps rather than rendering nothing.
7. `scripts/verify.sh quick` is green; the render-cost guard is unmodified.

## 14. Stop conditions

Report rather than improvise when:

- a surface's policy is not clamp or wrap but something the type cannot express
  — record it rather than adding a third policy for one caller;
- a conversion would change a page step or a jump behaviour;
- `McpDialog`'s dispatch turns out to share more with the simple group than
  expected, so that converting it is tempting — report the finding and leave the
  decision, because its `action_mode` selection is a different concept;
- the corpus moves for a control-case surface.

## 15. Closure evidence required

- The type and its boundary tests.
- The control-case byte-identical result.
- The per-surface policy tests.
- The non-adoption list with reasons.
- `quick` output.
- A closure record at
  `plans/closure/tui-modularity-and-render-invariants/009-status.md`.

## 16. Handoff notes

Sixteen copies of `select_up` look like one function and are three. The plan
names all three: identical clamp, clamp plus scroll-follow, and `McpDialog`'s
mode dispatch. Converting the third into the first would be the kind of
"cleanup" that changes behaviour while the diff looks smaller.

Compose `CenteredScroll`, do not replace it. It is already adopted by twelve
files and its centring rule is a deliberate choice — centring rather than
edge-pinning. Reimplementing it inside `ListCursor` would fork the rule the
moment someone changes one of them.

The policy parameter is the point. Sixteen hand-written versions do not disagree
because someone chose badly; they disagree because the choice was implicit. Make
it explicit at the call site and the next surface inherits an answer.
