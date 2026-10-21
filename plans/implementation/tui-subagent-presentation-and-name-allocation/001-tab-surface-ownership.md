# TUI Subagent Presentation and Name Allocation — Milestone P001 — The Subagent Tab Owns Its Pixels

Status: closed — closure `plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md`

Outcome: the viewport rect now has exactly **one** owner, chosen by
`App::viewport_owner()` — the route's own view, the change review, or the open
subagent tab — and rendered by exactly one branch, so "one surface, one rect"
is a fact about the code rather than about the order two calls happen in. The
change review outranks the tab, because it is a modal decision surface. The
subagent view fills its rect with the theme background before decorating it and
renders into the rect's **full** width; the tab strip's carved-out row carries
the theme background too, and its active entry now wears the same
`primary`-on-`selection` treatment the project tab strip gives its active
entry. Six `docs/tui.md` claims that promised a transcript now describe a run
inspector, because a subagent tab cannot show one — the projection carries no
conversation for a delegated run.

**Two of the plan's five required tests were vacuous as written** and were
caught only by running them against the unfixed code: the ghosting test's
15-character marker was hidden behind the subagent view's first line, and the
last-column test's background-opacity form was already satisfied by the
transcript's own background block. Both were rewritten, and both now fail
against the baseline. The closure record's §4.2 holds the evidence.

Repository baseline: `31a412844fbbf36055b29245075121035eefc385`

Source roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P001)

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles` — §4.4 *Frontends render projections*, §4.7 *Correctness before transparent magic*.
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`

Applicable ADRs: none.

Primary class: **invariant** (delivering I2 and I3)

## 1. Objective

When a subagent tab is open, the main buffer's viewport contains the subagent's own view and nothing else, and the tab strip row is opaque.

Today the subagent view is drawn **over** the main transcript instead of **instead of** it, so the reader sees the main agent's conversation showing through; and the tab strip paints foreground only over a row carved out of the viewport, so it is the one surface in the TUI with no background at all.

## 2. Why this milestone is ready

No hard dependencies. P001 touches only `src/tui/app/render.rs`, its tests, and `docs/tui.md`. It changes no projection, protocol, storage, or execution path, so `scripts/check_execution_ownership.py`, `scripts/check_scheduler_bypass.py`, and `scripts/check_tui_transport_authority.py` are unaffected.

## 3. Current implementation evidence

> This section is the **baseline record** at `31a4128`, kept as the plan wrote
> it. Its line numbers are pinned to that SHA and `src/tui/app/render.rs` has
> moved since the milestone landed. The closure record —
> `plans/closure/tui-subagent-presentation-and-name-allocation/001-status.md`
> — describes the state after the change, and its §3.1 records the deviations
> from this plan, including a **sixth** `docs/tui.md` transcript claim that
> this plan's §3.4 did not enumerate.

### 3.1 The overlay

`src/tui/app/render.rs:331` renders the viewport; `:337` then renders the subagent view into the **same rect**:

```rust
self.render_viewport(frame, viewport_chunk);
// An open subagent buffer replaces the transcript in the
// same rect rather than opening a modal over it: Esc then has
// exactly one meaning — return to the main buffer — instead of
// being ambiguous between "close a dialog" and "go back".
if self.ui_state.subagents.active_tab().is_some() {
    self.render_subagent_view(frame, viewport_chunk);
}
```

The comment states the intended behaviour. The implementation does not perform it. `render_subagent_view` (`render.rs:1502-1618`) renders a short paragraph of `Span`s — no background block — into `Rect { width: area.width - 1, ..area }` (`render.rs:1613-1617`). The transcript painted underneath therefore remains visible **beneath** the paragraph and in the **final column** of the rect.

`render_viewport` (`render.rs:896-915`) hands the viewport to the **change review** when one is open (`render.rs:901-908`) — a modal decision surface. Because the subagent view is drawn after it, the subagent view overlays the review too. That is the same defect class: two surfaces each believing they own the viewport.

### 3.2 There is no transcript to show

The tab cannot show a conversation, because nothing carries one:

- `AgentRunSummary` (`crates/codegg-protocol/src/projection/dto.rs:544-550`) documents that it "contains no prompt, transcript, mailbox body, hidden reasoning, credentials, or raw logs".
- `RunDetailView` (`crates/codegg-core/src/run_store.rs:967-977`) is a manifest — cell, invocation, permissions, and the canonical ids needed to reach durable run detail.

The tab is a **run inspector**, and its body already says so in substance: `Task`, `Agent` (the pool's persona), `Identity`, and the liveness statement ("This run is no longer in the session's projection.").

### 3.3 The transparent row

`render_subagent_tab_strip` (`render.rs:1631-1668`) paints **foreground only** — every `Style` in it sets `.fg(...)` and none sets `.bg(...)`.

The row it paints is carved out of the viewport chunk (`render.rs:307-319`), and the transcript's opaque background block is painted only over the viewport chunk (`render.rs:1235-1236`). The home renderer states the convention in a comment at `render.rs:1189-1190`: *"Render a background block first to ensure the viewport isn't transparent"*. The project tab strip applies the treatment to its entries (`render_tab_strip`, `render.rs:2150`, sets `.bg(theme.selection)` / `.bg(theme.background)`).

The tab row is therefore the one region of the main buffer with no opaque fill.

### 3.4 The false documentation

| Location | Claim |
|---|---|
| `docs/tui.md:671` | "One subagent's **transcript** per tab, in the main buffer" |
| `docs/tui.md:739` | "A subagent tab shows that subagent's **transcript**." |
| `docs/tui.md:273` | "`Enter` opens a session or a subagent **transcript**" |
| `docs/tui.md:687` | "`Enter` opens a child **transcript** or job detail" |
| `docs/tui.md:785` | "a **subagent** row opens that subagent's **transcript** as a tab in the main" |

`architecture/tui.md:772-776` is already correct on the rect replacement and on the tab being the full-fidelity `Task`/`Agent` surface, and must not be changed by this milestone beyond wording that refers to the transcript.

## 4. Invariants that must not regress

- **I1 — Projection-derived rows.** The subagent view renders only `SubagentRow` fields that arrived in the projection. Do not synthesize progress text: the projection adapter hardcodes `progress: None`, so an invented "working on step 3" line would be a lie the user acts on (`render.rs:1496-1501`).
- **I4 — Panel input authority.** `Esc` must keep exactly one meaning while a tab is open: return to the main buffer. Do not introduce a modal or a second dismiss path.
- **I6 — Tab ordering is not an execution boundary.** A tab cancels nothing.
- The **working strip row 0** stays reserved and stays the main buffer's only status surface.
- The change review remains **modal** and keeps taking precedence over the subagent view.
- `render_viewport`'s panic-catching wrapper (`render.rs:328-341`) keeps its current shape: a panic inside the subagent view must still be caught and latched into `last_render_error`.

## 5. Scope

### In scope

- The viewport rect when a subagent tab is active.
- The tab strip row's background.
- The tab body's width.
- The precedence between the subagent view and the change review.
- The `docs/tui.md` transcript claims.

### Explicitly out of scope

- The tab strip's **content** — label, glyph, persona, key hints. That is P002.
- Adding a transcript. Out of scope entirely: nothing carries one.
- The side panel, the `Ctrl+T` overlay, and the `/sessions` dialog.
- The name allocation policy. That is P004.

## 6. Required production changes

### Frontend surface

1. **Establish who owns the viewport.** Introduce one explicit decision in `render()` for the viewport rect: the subagent tab owns it when a tab is active **and** no change review is open; otherwise the change review owns it; otherwise the transcript owns it. Render exactly one owner. The current structure — render the viewport, then render the subagent view over it — must not remain, because it cannot express "exactly one owner" and it draws two.

   The subagent tab is not modal and must not become modal: `Esc` returns to the main buffer (`render.rs:1608-1611`), and that behaviour is load-bearing.

2. **Paint the background.** When the subagent tab owns the viewport, paint an opaque background block over the **whole** viewport rect before the subagent view, following the convention at `render.rs:1189-1190` and `render.rs:1235-1236` (`Block::default().style(Style::default().bg(self.ui_state.theme.background))`). The block must cover the full rect, not the paragraph's own area.

3. **Fill the rect.** The subagent view must render into the full width of the rect. The `width - 1` at `render.rs:1613-1617` leaves the last column unpainted; remove it and let the paragraph use `area` unchanged. If a one-column inset is wanted for aesthetics, it must be painted as a background, not left as a gap.

4. **Make the strip row opaque.** `render_subagent_tab_strip` must paint a background over its rect before its spans, using the theme's background — and, if the active entry is to read as selected, the same treatment `render_tab_strip` gives its entries (`render.rs:2150`). Keep the entry foreground styling as it is.

   A row carved out of a viewport chunk is still a row. The block must be painted, not merely the text styled.

### Documentation and static guards

- Correct the five `docs/tui.md` claims listed in §3.4 so they describe a **run inspector**: the tab shows the subagent's task, its agent description, its canonical ids, and its liveness — not a transcript.
- Leave `architecture/tui.md`'s correct statements alone; adjust only wording that says the tab shows a transcript.
- No new static guard is required. The defect is a rendering fact, and a **test** is the right evidence — a line-pattern guard over `render.rs` would be exactly the fragile check `plans/003-planning-process.md` §11 warns against.

## 7. Ordered work packages

### Work package A — Decide and enforce one viewport owner

Intent: make "exactly one surface owns the viewport" a structural fact rather than an ordering accident.

Required changes: restructure the viewport rendering so the subagent tab, the change review, and the transcript are **alternatives**, not layers. Keep the panic-catching wrapper and its `last_render_error` latching intact.

Acceptance evidence: a render test that opens a subagent tab over a transcript containing a distinctive marker string, and asserts the marker is **absent** from the rendered buffer while the subagent's own title is **present**.

### Work package B — Paint the tab body and the strip row

Intent: no transparent cell in the main buffer.

Required changes: the opaque background block for the viewport when the tab owns it; the opaque background for the strip row; the full-width paragraph.

Acceptance evidence: a render test asserting (a) the strip row's cells carry a background style, and (b) content appears in the **last column** of the viewport rect — proving nothing is left unpainted at the edge.

### Work package C — Correct the documentation

Intent: stop promising a capability the product does not have.

Required changes: the five `docs/tui.md` claims.

Acceptance evidence: a diff review showing each claim now describes the run inspector. No test; this is a prose correction and must be stated in the closure record as such.

## 8. Failure, cancellation, restart, and contention semantics

- A panic inside the subagent view must still be caught by the existing wrapper and latched into `last_render_error`; the frame must not abort.
- A tab whose run has left the projection keeps rendering (the liveness statement at `render.rs:1546-1553`); this milestone must not turn that into an error state.
- No cancellation, restart, or contention behaviour changes. The milestone adds no state and no async work.

## 9. Compatibility and migration

None. No schema, protocol, configuration, or storage change. Old snapshots render exactly as they do now, except that they are no longer visible through an open tab.

## 10. Required tests

### Focused unit tests

- A test that the viewport's owner selection returns the subagent view when a tab is active and the review is closed, the review when it is open, and the transcript otherwise.
- A test that the strip's base style carries a background.

### Integration / render tests

- **The ghosting test.** Render a session transcript containing a unique marker (e.g. `ZEBRAFISH-MARKER`), open a subagent tab, and assert the marker does not appear anywhere in the rendered buffer. This is the acceptance test for the reported defect.
- **The last-column test.** Assert that the subagent view's content occupies the final column of the viewport rect.
- **The opacity test.** Assert that every cell of the tab strip row carries a background style.
- **The review-precedence test.** With a change review open and a subagent tab active, assert the review's content is the content that is visible — i.e. the tab did not overlay it.

### Negative tests

- With **no** subagent tab open, assert the transcript renders normally and the marker **is** present (proving the ghosting test is not vacuous).

### Security and negative tests

None specific; no new input path is introduced.

## 11. Required verification commands

```bash
# narrow tests first
cargo test -p codegg --lib tui::app::render
cargo test -p codegg --test tui_render

# static guards
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh quick
```

Do not claim any command that was not actually run.

## 12. Documentation updates

- `docs/tui.md:671`, `:273`, `:687`, `:739`, `:785` — describe the run inspector, not a transcript.
- `architecture/tui.md` — adjust only wording that claims a transcript.
- `docs/tui.md` §"Subagent tabs" — state that the tab **replaces** the main transcript in the viewport (the behaviour this milestone makes true) and that the strip row is opaque.

## 13. Acceptance criteria

1. With a subagent tab open and no change review open, the main transcript is **not visible** anywhere in the viewport rect.
2. With a change review open, the change review is visible and is **not** overlaid by the subagent view.
3. The subagent view renders into the full width of the viewport rect.
4. The tab strip row carries an opaque background; no cell of the main buffer is transparent.
5. `docs/tui.md` makes no claim that a subagent tab shows a transcript.
6. `scripts/verify.sh quick` is green, and the four named guards pass.

## 14. Stop conditions

Stop and report rather than improvise when:

- Making the tab own the viewport would require making it modal, or would change the meaning of `Esc`.
- The opaque background would have to be painted over a surface that legitimately owns the viewport (the change review) — the precedence in §6 must be settled first, not worked around.
- The fix appears to require a transcript, a projection change, or a new protocol field. It does not; the tab is a run inspector.
- The viewport restructure would change the working strip's reserved row 0.

## 15. Closure evidence required

- The implementation SHA.
- The requirement-to-evidence matrix: each of the six acceptance criteria mapped to the named test that proves it.
- The exact commands run and their results, including `scripts/verify.sh quick`.
- An explicit statement of what was **not** run (hosted CI, a live TUI session).
- The documentation diff for `docs/tui.md`.

## 16. Handoff notes

- The reported symptom is "the buffer does not belong to the selected subagent". Its cause is an overlay, not a wrong-data bug — do not go looking for a data problem.
- The fix is in `src/tui/app/render.rs` only. `src/tui/app/state/subagent_tabs.rs` and the projection are untouched by this milestone.
- P002 will edit the same file (`render_subagent_tab_strip`) and the same strip row. Do not implement P002's content here — but do leave the strip's painting structured so that a background can be added without restructuring the span loop again.
- `cargo test -p codegg` compiles the whole crate; the lib test filter above is the narrow target.
