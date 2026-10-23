# Repository Audit Corrective Milestone C006 — TUI Render-Path and Transcript Cost

Status: ready for handoff

Repository baseline: `d5be7acd05dc999888430b7ad23420d554359fec` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c006--tui-render-path-and-transcript-cost`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `architecture/tui.md`, `architecture/testing.md`

Applicable ADRs: none required. No contract, ownership boundary, or protocol changes. The affected subsystems' closed milestones carry named conditions that this milestone must check, and it must not reopen them silently.

Primary class: **polish**

## 1. Objective

The composer, the streaming transcript, the tasks overlay, the editor's highlighter, and the file tree stop re-deriving the same result more than once per frame, and the primary transcript stops retaining a second rendered copy of itself indefinitely.

## 2. Why this milestone is ready

Its hard dependency is that **C001 and C003 have closed**. Both change the TUI command and render path, and C003 in particular changes the dispatcher's argument handling, which is on the same path as the composer and the streaming surface. Measuring and changing cost before the tree is stable would produce a measurement of the wrong code.

It is sequenced after the correctness milestones for a second reason, which is a rule and not a preference: a polish phase created before the capability's correctness boundary is closed is a prohibited planning anti-pattern. The correctness boundary of this workstream is C001–C004.

C002, C004 and C005 do not block it.

## 3. Current implementation evidence

### The render loop is not the problem, and this milestone must not change it

`src/tui/runtime/event_loop.rs` is well built, and the audit recorded the following as **not findings**:

- a dirty flag (`needs_render`) gates redraw, so the frame is not repainted when nothing changed;
- the render interval is zero when not streaming and 16 ms when streaming, so there is no 60 fps redraw;
- the event wait is a bounded select with a timeout, so the loop does not busy-wait;
- there is one frame source and one `ActivityPhase`;
- the LSP status is fetched off the render path and cached;
- the render is wrapped in a `catch_unwind`, and a panic in rendering degrades rather than terminates.

Every cost this milestone addresses is in **content derivation**, not in loop discipline. A change that alters the frame cadence, the spinner cadence, or the catch-unwind boundary is out of scope and would be a regression against `architecture/tui.md`.

### The streaming tail is re-parsed as Markdown on every frame

`src/tui/components/messages.rs` renders the streaming tail with:

```rust
rows.extend(render_markdown(
    &self.streaming_tokens,
    &self.theme,
    self.theme.muted,
    tail_width,
));
```

and `render_markdown` builds a fresh `Arena`, runs a full comrak `parse_document` over the text, and renders the resulting block tree:

```rust
let arena = Arena::new();
let root = parse_document(&arena, text, &MARKDOWN_OPTIONS);
```

`self.streaming_tokens` is the **entire accumulated response so far**. So each frame during a response re-parses the whole response as Markdown. With a response of length *L* arriving over *k* frames, the work is *O(L·k)* — quadratic in the response length, paid on the UI thread, on the path that must keep up with typing.

The same shape appears in the streaming message's wrap path: `src/tui/runtime/app_events.rs` invalidates and re-wraps the streaming message on every delta, and the re-wrap is over the whole message, so the cost is *O(L²)* over a response.

### The composer re-wraps its entire buffer four times per frame

`src/tui/components/prompt.rs` computes the composer's wrapped layout, and `src/tui/app/render.rs` calls into it from at least four places in a single frame — the layout pass, the pre-draw pass, the draw itself, and the post-draw caret pass. Each call re-wraps the **whole** composer buffer. There is no wrap cache, and the composer has **no size cap**.

The consequence compounds: the composer is the one surface the user is actively typing into, and its buffer is the only input that changes on every keystroke.

### The composer text is cloned wholesale, twice per interaction

`src/tui/components/prompt.rs` clones the whole composer text on every frame to test whether it starts with `!`. `src/tui/app/render.rs` clones it again during the frame, and `src/tui/app/mod.rs` clones it again inside the completion update, which runs **per keystroke**. The completion path's clone is the expensive one: it is on the keystroke path, and it scales with the buffer.

### The editor's highlight cache is invalidated by every keystroke

`src/tui/editor_highlight.rs` keys its syntax-highlight cache on the **document revision**. A local edit bumps the revision, so the whole cache is discarded and the visible lines are re-scanned from zero on every keystroke. The cache therefore provides no benefit at all on the path it was built for — the one that matters — while still costing a lookup and a hash on every keystroke.

### The tasks overlay builds everything, then throws most of it away

`src/tui/app/render.rs` asks the tasks overlay for its rows, and `src/tui/app/mod.rs` builds the **full, unbounded** list of task rows and **deep-clones** it — and then the render truncates the result to the twelve rows that fit. The work is proportional to the total number of tasks, and it is done twice per frame.

### The transcript is never trimmed, and is held twice

`src/tui/components/messages.rs` keeps the primary transcript vector with **no cap**, and the layout cache holds a **second** rendered copy of the same content. The sibling chat surface in `src/tui/app/state/chat.rs` caps at one hundred messages per channel, so the two surfaces in the same subsystem apply different retention policies.

The consequence is that a long session's memory grows with the session, and the layout cache doubles it.

### Smaller costs on the same path

- The file tree rebuilds and re-clones its whole row list two to three times per frame, and lowercases every name and path on every filter pass. Its `selected_index` lookup rebuilds the entire list to find one index.
- `row_cells` in `src/tui/composer_geometry.rs` is quadratic in the number of cells when the slice contains an escape sequence, and the caret lookup is a linear scan over all cells.
- The `/search` path clones every message part two to three times, and then invalidates the whole layout cache.
- `ComposerText::point_at` counts newlines from byte zero, so the caret's position is recomputed over the whole buffer on each caret change.

## 4. Invariants that must not regress

- **The frame cadence is unchanged.** One spinner cadence, one `ActivityPhase`, one frame table, and the streaming render interval of 16 ms stay as they are. This milestone changes what a frame *computes*, not how often frames happen.
- **The render is still wrapped in a `catch_unwind`,** and a panic in rendering still degrades rather than terminates.
- **The visible result is unchanged.** Every measurement must be of the same rendering. A change that alters what the user sees is a different milestone, not an optimisation.
- **The main buffer's layout rules are unchanged.** The main buffer has no status row of its own, and nothing is reserved between the transcript and the composer, so the transcript cannot reflow when the turn state changes. That rule is load-bearing and this milestone must not disturb it.
- **The composer's visible-row geometry is unchanged.** The wrap width is the pane's width, and the height is a budget rather than a row cap.
- **The editor's wrap primitive keeps its contract.** The styled, byte-offset-preserving primitive loses neither styles nor byte offsets, and the OSC-8 hyperlink sequence must survive whole.
- **The transcript's ordering and content are unchanged.** A cap, if one is added, must not change which messages the user can see or scroll to.
- **The LSP status stays off the render path.**

## 5. Scope

**In scope**

- The streaming tail's Markdown parse, and the streaming message's wrap.
- The composer's wrap and layout computation, and the composer's per-frame and per-keystroke clones.
- The editor's highlight cache key.
- The tasks overlay's row construction and clone.
- The primary transcript's retention, and the layout cache's duplicate copy.
- The file tree's row cache and its selection lookup.
- `row_cells`' escape-sequence handling and the caret lookup.
- The `/search` path's clones and its cache invalidation.
- `ComposerText::point_at`'s caret-position computation.
- A measurement recorded for each change.

**Out of scope**

- The event loop, the frame cadence, the spinner, and the `catch_unwind` boundary.
- The main buffer's wrap primitive, the composer's wrap primitive, and the visual-row geometry. Those are owned by the `tui-main-buffer-text-rendering` and `tui-composer-text-layout` lines, whose milestones are **conditionally closed with named conditions**. C006 must read those named conditions and confirm it is not reopening them; if a change here would alter their contract, that is a stop condition.
- The disclosure and collapse model.
- Any user-visible rendering change, including the number of tasks shown in the overlay and the number of transcript rows retained.
- A benchmark harness. Without one, no wall-clock claim is made, and creating one is a separate milestone.

## 6. Required production changes

### Core

- The streaming tail's Markdown must be parsed **incrementally or cached**, not re-parsed in full per frame. The implementer must choose between a cache keyed on the accumulated token length, an incremental parse, and a bounded re-parse of only the changed tail. Whichever is chosen, the rendered result must be identical.
- The composer's wrapped layout must be computed **once per frame** and shared between the four call sites, and must be **cached against the buffer's content and the width**, so that a frame with no edit does no wrapping work.
- The composer text must not be cloned wholesale to answer "does it start with `!`". The implementer must find a way to answer that from a borrowed reference.
- The completion path must not clone the whole buffer per keystroke.
- The editor's highlight cache must be keyed on something that a local edit does not change — for example the document content and the visible range, rather than the revision.
- The tasks overlay must produce only the rows it will draw, or at minimum must not deep-clone the full list before truncating.
- The transcript must have a **stated** retention policy, consistent with the sibling chat surface's, and the layout cache must not hold a second full copy of the transcript.
- The file tree's row list must be built once and reused, and its selection lookup must not rebuild the list.
- `row_cells` must not be quadratic in the presence of an escape sequence.
- `ComposerText::point_at` must not scan from byte zero on every caret change.

### Storage

None. The transcript's retention is an in-memory policy. If the implementer finds that retention currently depends on a persisted value, that is a finding to record rather than to change here.

### Protocol

None.

### Runtime

None. Nothing in this milestone changes what runs where.

### Frontend

- The rendered output must be **byte-identical** to the baseline for a given state. This is the milestone's primary acceptance property, and it is checkable: a render comparison over a fixture set of states.

### Security

No change to an authorization boundary. One consequence to check rather than assume: the transcript's retention change alters what a session keeps in memory, and the file tree's row cache holds **file paths**. The implementer must confirm the file tree's cached rows are invalidated when the tree changes, so a stale path is not displayed, and must confirm no path is retained beyond the session that owns it.

### Docs

- `architecture/tui.md` must record the caching strategy for the composer layout, the streaming parse, and the transcript retention, at the level of the invariant rather than the mechanism.

## 7. Ordered work packages

### WP1 — Build the measurement baseline

Intent: every subsequent claim needs a before.

Required changes: instrument or otherwise capture, for a representative set of states — a long response streaming, a long composer buffer, a large task list, a large file tree, a long transcript — the number of wrap computations, parse calls, and full-buffer clones per frame, and the wall-clock cost of a frame where it can be measured.

The repository has no benchmark harness for the TUI. The implementer may add a **test-only** measurement, or may rely on the mechanism counts alone, and must say which. The mechanism counts are the more reliable evidence here, because they are deterministic.

Acceptance evidence: the baseline counts are recorded per state, before any change.

### WP2 — Cache the streaming parse and wrap

Intent: stop re-parsing the whole response per frame.

Required changes: as §6 describes, with the rendered result unchanged.

Acceptance evidence: the per-frame parse count for a streaming response is bounded by the change, not by the response length, and the rendered output is identical.

### WP3 — Share the composer's layout

Intent: one wrap per frame, and none when nothing changed.

Required changes: compute the composer's wrapped layout once per frame and pass it to the four consumers; cache it against the buffer content and the width.

Acceptance evidence: the per-frame wrap count for the composer is one, and zero when the composer is unchanged and the width is unchanged.

### WP4 — Remove the whole-buffer clones

Intent: stop cloning the composer text per frame and per keystroke.

Required changes: answer the `!` check without a clone; remove the completion path's clone.

Acceptance evidence: the per-frame and per-keystroke clone counts for the composer text are zero, and the completion behaviour is unchanged.

### WP5 — Fix the highlight cache key

Intent: make the cache useful on the path it was built for.

Required changes: re-key the cache so a local edit does not discard it, and confirm the cache is invalidated when it must be — when the document content changes, when the visible range changes, or when the language changes.

Acceptance evidence: a keystroke no longer invalidates the whole cache, and the highlighted output is identical.

### WP6 — Bound the tasks overlay

Intent: stop building and cloning what will be discarded.

Required changes: produce the rows that will be drawn.

Acceptance evidence: the overlay's row construction is proportional to the rows drawn, not to the total task count, and the visible rows are unchanged.

### WP7 — State the transcript's retention, and stop the second copy

Intent: a bounded, stated memory policy.

Required changes: give the primary transcript the same kind of retention the sibling chat surface has, or a stated reason why it differs; and stop the layout cache from holding a second full copy.

The retention policy is a **user-visible** decision — a cap changes what a user can scroll back to. The implementer must therefore either adopt a cap that is consistent with the sibling surface and record it as the policy, or demonstrate that the transcript must be unbounded and record why. The implementer must not choose silently.

Acceptance evidence: the policy is stated and recorded; the memory held for a long session is bounded or the unboundedness is justified; the user can still reach every message the policy says they can reach.

### WP8 — Fix the file tree and the smaller costs

Intent: the remaining per-frame work.

Required changes: the file tree's row list is built once and reused, with a correct invalidation; the selection lookup does not rebuild the list; `row_cells` is not quadratic with an escape sequence; the caret lookup is not a linear scan over all cells; `/search` does not clone every message part; `ComposerText::point_at` does not scan from byte zero per caret change.

Acceptance evidence: each is measured before and after, and the rendered output is identical.

### WP9 — Prove the rendering is unchanged

Intent: the milestone's primary acceptance property.

Required changes: a render comparison over a fixture set of states — empty, typical, long composer, long transcript, streaming, large task list, large file tree, content with escape sequences and hyperlinks — asserting that the rendered output is identical to the baseline. The OSC-8 hyperlink sequence must survive whole, and the wrap must produce the same rows.

Acceptance evidence: the comparison passes, and the fixture set is committed.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A cache that is stale is worse than no cache: the user sees content that is not there. Every cache introduced here must have an invalidation that is a function of what the cached value depends on, and the invalidation must be proven by the render comparison in WP9.
- **Cancellation.** A cancelled session must release its cached state. The transcript's retention change must not leave a session's content reachable from another session's cache.
- **Restart.** Nothing durable changes. A restart reconstructs the state from the store, and the caches are in-memory, so they start empty.
- **Contention.** The caches are per-session and per-surface. They must not be shared across sessions, and the file tree's row cache must not outlive the tree it describes.

## 9. Compatibility and migration

- **User-visible:** the only intended user-visible change is the transcript's retention, if a cap is adopted. That is recorded as a policy, not as a silent side effect. Everything else must be invisible.
- No protocol, storage, or configuration change.
- No migration.

## 10. Required tests

**Focused**

- The render comparison in WP9.
- A test that the composer layout is computed once per frame and is reused across the frame's consumers.
- A test that a local edit does not discard the highlight cache, and that a content change does.
- A test that the tasks overlay's construction is proportional to the rows drawn.
- A test that the transcript's retention is the stated policy.
- A test that the file tree's row cache is invalidated when the tree changes.

**Integration**

- A test that drives a streaming response through the render path and asserts the parse count is bounded and the output is identical.

**Security**

- A test that a stale file-tree row is never displayed after the tree changes.

**Migration**

- None.

**Placement.** The `tui_render` family is the right home. New members are `tests/tui_render/<name>.inc`, bound in `tests/tui_render/mod.rs` with `#[path = "<name>.inc"] pub mod <name>;`. The family target is already declared in the root `Cargo.toml`; a new family would need that declaration and is not expected here.

**Vacuity.** The render comparison must be shown failing when a rendering is deliberately perturbed, so that it is known to be capable of detecting a difference. A comparison that cannot fail is not evidence.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The affected family.
cargo test --test tui_render

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry); the implementer must record the failure set before and after and state that the change added and removed no lint finding. Linux must stay green.

## 12. Documentation updates

- `architecture/tui.md`: the caching and retention invariants.
- `docs/`: no change, unless the transcript's retention policy is user-visible, in which case it is documented there too.

## 13. Acceptance criteria

Externally observable:

1. For a streaming response, the per-frame Markdown parse count is bounded and does not grow with the response length.
2. For a frame with an unchanged composer, the composer is wrapped once, not four times.
3. No keystroke clones the whole composer buffer.
4. A local edit no longer discards the editor's highlight cache.
5. The tasks overlay's construction cost is proportional to the rows it draws.
6. The transcript's retention is stated and recorded, and the layout cache does not hold a second full copy.
7. The rendered output is identical to the baseline for every state in the fixture set.
8. The frame cadence, the spinner, and the render's `catch_unwind` are unchanged.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- A change would alter the rendered output. Stop: this milestone is an optimisation, and a rendering change is a different milestone.
- A cache cannot be given a correct invalidation, or the invalidation cannot be proven by the render comparison. Stop: a stale cache is a correctness defect, and the trade is not worth it.
- A change would reopen a **conditionally closed** milestone in the `tui-main-buffer-text-rendering` or `tui-composer-text-layout` lines, or would alter the contract of their named conditions. Stop and report which condition is affected.
- A change would alter the frame cadence, the spinner cadence, the `ActivityPhase` set, or the render's `catch_unwind` boundary. Stop: `architecture/tui.md` makes these contracts.
- The transcript's retention would become bounded in a way that removes a message the user could previously reach, and that is not acceptable as a policy. Stop: the implementer must record the decision, not make it silently.
- The file tree's row cache cannot be invalidated correctly. Stop: a stale path displayed to the user is a correctness defect.

## 15. Closure evidence required

- The WP1 baseline and the after-measurements, per change, with the measurement method stated.
- The render comparison's result, and the demonstration that it can detect a difference.
- The transcript's retention policy, recorded, and the registry or architecture note that carries it.
- The confirmation that the frame cadence, the spinner, and the `catch_unwind` are unchanged, verified against the diff.
- The `verify.sh scoped` and `verify.sh full` results, labelled local versus CI, with the macOS lint set stated before and after.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- **Do not touch the event loop.** The audit found the loop to be sound. A change to the frame cadence, the spinner, or the catch-unwind boundary is a regression against `architecture/tui.md`, and this milestone has no reason to go there.
- The **wrap and the parse are different costs** and are separate work packages. The streaming tail is re-parsed as *Markdown*; the streaming message is re-*wrapped*. Both are whole-content operations on the UI thread, and both must be fixed, but they are not the same fix.
- The composer's four call sites are **layout, pre-draw, draw, post-draw caret**. They need one shared computation, not four faster ones.
- The highlight cache's problem is its **key**, not its size. A cache keyed on a value every keystroke changes is not a cache on the path that matters.
- The transcript's retention is a **user-visible** decision. The sibling chat surface already caps at one hundred per channel; the primary transcript does not. Adopting a cap is a policy statement and must be recorded as one.
- The strongest acceptance evidence here is the **render comparison**: identical output, with a demonstrated ability to detect a difference.
- This milestone closes no other milestone. C007 depends on it, because C007's comment budget must be initialised from the final shape of the tree.
