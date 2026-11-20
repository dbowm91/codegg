# TUI Render Cost Milestone M003 — The Chat and Change-Review Row Models

Status: ready for handoff

Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Source roadmap:

- `plans/subsystems/tui-render-cost-roadmap.md#7-milestones` (M003)

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`
- `architecture/tui.md` — "One chat browser, two surfaces", "Chat side panel",
  and "The render path's caches (C006)"
- `architecture/tui.md` §"The transcript's retention" — the sibling-surface
  distinction this milestone must not blur

Applicable ADRs: **none required.**

Primary class: **polish** (cost)

## 1. Objective

The project chat wraps each message body **once per content change** rather than
once per draw, and the change review **borrows** its parsed diff rather than
deep-cloning it per frame.

## 2. Why this milestone is ready

No hard dependency. The invalidation primitives already exist and are already
owned by the chat reducer: `ProjectChat::sequence` is a local monotonic counter
bumped on every applied page or event, and `ChatMessageDto` carries its own
`revision` and `redacted` flag. The change review's fix needs no key at all —
the state already owns the hunks; only the render path copies them.

## 3. Current implementation evidence

Line numbers are at the baseline. Re-derive them.

### 3.1 The chat row model is rebuilt from scratch on every draw

`src/tui/chat_browser.rs:479`:

```rust
pub fn rows(view: &ChatView<'_>, cursor: usize, width: u16, theme: &Theme) -> Vec<ChatRow> {
    let width = width.max(12);
    let mut out: Vec<ChatRow> = Vec::new();
    let unread_from = unread_boundary(view).unwrap_or(usize::MAX);
    for (index, message) in view.messages.iter().enumerate() {
        ...
        let chunks = if message.redacted {
            vec!["[redacted]".to_string()]
        } else {
            wrap_to_strings(&message.body, body_width)          // :504
        };
```

There is **no cache** anywhere in the chat surface: no `RefCell`, no `Cell`, no
revision field on any of `src/tui/chat_browser.rs`, `src/tui/panels/chat.rs`,
`src/tui/components/dialogs/chat.rs`, or `src/tui/app/state/chat.rs` that a
cache could key on. Every draw flattens the whole retained window — up to
`MAX_CHAT_MESSAGES_PER_CHANNEL = 100` (`src/tui/app/state/chat.rs:52`) — into
fresh `ChatRow`s whose `Line<'static>` fields force every wrapped chunk to be an
owned `String`.

Two call sites, both on the frame:

- `src/tui/panels/chat.rs:349` — the docked chat panel;
- `src/tui/components/dialogs/chat.rs:390` — the `/chat` box.

**The amplifier.** The chat surface's own display cap is **not** applied on this
path. `MAX_CHAT_BODY_DISPLAY_LEN = 500` (`state/chat.rs:58`) is enforced only in
`message_line` (`:1573`), which is the command-line projection. So a body up to
the daemon's bound is wrapped in full, at sidebar width (~28 columns), producing
hundreds of rows per message. The cost is proportional to
**total retained history × body size**, not to the viewport.

### 3.2 The unread count is a full window scan per frame

`src/tui/app/state/chat.rs:246`:

```rust
pub fn unread_count(&self) -> usize {
    if !self.read_marker_known { return 0; }
    self.messages.iter().filter(|m| m.seq > self.last_read_seq).count()
}
```

Called from `ChatPanel::view` (`src/tui/panels/chat.rs:120`), which runs on every
panel draw and again on the key paths. The answer changes only when a message is
applied or the read marker advances.

### 3.3 The viewport is derived by cloning visible rows and scanning all rows

`src/tui/panels/chat.rs:362`–`:364`:

```rust
let selected_end = chat_browser::selected_row_end(&lines, self.cursor());
let range = chat_browser::window(lines.len(), selected_end, body_height);
let window: Vec<Line> = range.map(|index| lines[index].line.clone()).collect();
```

`selected_row_end` (`chat_browser.rs:627`) reverse-scans **all** rows, not the
viewport. The clone itself is bounded by the viewport, but it is a second deep
copy of rows that were just allocated. The dialog path does the same at
`dialogs/chat.rs:406`–`:408`.

### 3.4 The change review deep-clones the parsed diff every frame

`src/tui/components/change_review.rs:140`:

```rust
let mut viewer = DiffViewer::new(
    String::new().into_boxed_str(),
    String::new().into_boxed_str(),
    patch.path.clone().into_boxed_str(),
);
viewer.hunks = patch.hunks.clone();
viewer.render_in(body, frame.buffer_mut(), &theme);
```

`DiffViewer` (`src/tui/components/diff.rs:36`) owns `hunks: Vec<DiffHunk>`, and
`render_in` (`:286`) reads only `self.hunks` (`:136`, `:200`, `:345`). The
`compute_diff` inside `DiffViewer::new` is cheap here because the inputs are
empty, but the assignment copies the whole parsed patch — bounded by
`MAX_REVIEW_PATCH_BYTES = 512 * 1024` (`src/tui/unified_diff.rs:33`) — on every
frame the review is open.

## 4. Invariants that must not regress

- **The rendered output is byte-identical** for a given state. The chat rows,
  their wrapping, their indentation, the unread divider's position, the
  selection marker and the retention header are unchanged.
- **A redacted body stays redacted.** The cache must key on the message's
  `redacted` flag and `revision`, so a body that becomes redacted is re-rendered
  rather than served from a pre-redaction entry.
- **The cursor still highlights the whole message**, not one wrapped row.
- **`selected_row_end`'s rule is unchanged**: the selected message's last row is
  the anchor, and `window` still keeps it visible with context on both sides.
- **The chat window's retention is unchanged.** `MAX_CHAT_MESSAGES_PER_CHANNEL`
  is not this milestone's business, and applying `MAX_CHAT_BODY_DISPLAY_LEN` to
  the render path would be a **user-visible truncation** — a different
  milestone. See §5.
- **One producer of the rows.** The cache is the producer; no surface may build
  `ChatRow`s beside it.
- **The change review's scrolling, side-by-side toggle and theme behaviour are
  unchanged** (`DiffViewer`'s own logic is not re-derived).

## 5. Scope

### In scope

- A chat row cache behind `chat_browser::rows`, or a replacement accessor that
  borrows.
- `ChatPanel::view`'s `unread_count()` call.
- The viewport derivation in both chat surfaces.
- `ChangeReviewView::render`'s hunk clone, and a borrowed render path on
  `DiffViewer`.
- Counters for the chat row builds, the chat wrap passes, the unread scans, and
  the review's hunk copies.

### Explicitly out of scope

- **`MAX_CHAT_BODY_DISPLAY_LEN` on the render path.** Applying the 500-byte cap
  would change what the user sees in the chat panel. It is a plausible
  optimisation and it is **not this milestone's**: record it as a finding in the
  closure record and let a product-visible decision own it.
- The chat window's retention (100 messages) and its paging (`u`, backward
  paging, `resync_required`).
- The chat protocol, the reducer, the channel list, and the composing-lease row.
- `ChatState::snapshot()` (`state/chat.rs:594`) — it clones up to 100 message
  DTOs but is called from command/event paths, not the render path.
- The diff algorithm, `unified_diff.rs`, and the intra-line highlight.
- The transcript's retention and the main transcript (M004).

## 6. Required production changes

### Core/domain

- `chat_browser` must gain a row cache. The implementer must choose between:

  - **(a) A coarse key.** Cache the whole `Vec<ChatRow>` against
    `(ProjectChat::sequence, last_read_seq, width, cursor, theme identity)`.
    Small, safe, and sufficient: during streaming the cursor does not move, so
    the cache hits at frame rate; a scroll or a cursor move rebuilds at human
    speed. The cursor genuinely *is* an input to the styled rows, so keying on
    it is correct rather than lazy.
  - **(b) A fine key.** Cache per-message wrapped chunks against
    `(message_id, revision, redacted, body_width)`, plus a cumulative offset
    index against `(sequence, width)`, and apply the selection marker, the
    selection style and the unread divider at draw time from the offsets.

  **(b)** is the better end state because cursor movement then costs nothing,
  but **(a)** is the bounded change and is acceptable. Whichever is chosen, the
  accessor must hand back a borrow or an `Rc<[ChatRow]>`, not a fresh `Vec`, and
  the caller must stop cloning the visible rows a second time.

- `ChatPanel::view` must not compute `unread_count()` by scanning. Either the
  count is maintained incrementally by the reducer (bumped on apply, reset on
  mark-read), or it is memoized on `(sequence, last_read_seq)`. The incremental
  form is preferred; the memo is the fallback.
- The viewport derivation must use the cached offsets rather than
  `selected_row_end`'s full reverse scan. If (a) is chosen, `selected_row_end`
  may remain — it is bounded by the cached row count and runs only on a miss —
  but the second clone of visible rows must go.
- `DiffViewer` must gain a borrowed render path — e.g.
  `render_hunks_in(&self, hunks: &[DiffHunk], area, buf, theme)` — with
  `render_in` delegating to it with `&self.hunks`. `ChangeReviewView::render`
  then passes `&patch.hunks` and does not construct a viewer with empty content
  at all.

### Storage and migrations

None.

### Protocol and DTOs

None. `ChatMessageDto` already carries `revision` and `redacted`.

### Runtime and concurrency

None. The cache is owned by the surface, on the render thread.

### Frontend or operator surface

- Byte-identical rendering for the fixture set in §10.

### Security and authorization

The cache holds **message bodies**, including redacted ones. The implementer
must:

- prove that a message whose `redacted` flag becomes true renders `[redacted]`
  on the next frame, not its pre-redaction body (a test, not an argument);
- prove that an edited message (`revision` bumped, same `message_id`) re-renders;
- confirm the cache is per-project and per-channel, is dropped on channel
  switch, project switch and session teardown, and cannot serve one channel's
  rows to another.

### Documentation and static guards

- `architecture/tui.md` §"The render path's caches (C006)": one row for the chat
  row cache (its key and what drops it) and one for the review's borrowed hunks.
- `architecture/tui.md` §"One chat browser, two surfaces": the browser produces
  rows once; both surfaces read the same cached rows.
- `src/tui/render_cost.rs`: `chat_row_builds`, `chat_wrap_passes`,
  `chat_unread_scans`, `review_hunk_copies`.
- `tests/tui_render/tui_render_cost.inc`: an assertion per counter.
- `scripts/check_tui_render_cost.py`: a shape check that neither chat surface
  builds rows beside the cache, and that `ChangeReviewView::render` does not
  assign `hunks`. Add each to `--self-test`.

## 7. Ordered work packages

### WP1 — Baseline counts

Intent: a before for every claim.

Required changes: add the four counters with no behaviour change; run the new
assertions against the unmodified tree and record the failures.

Acceptance evidence: baseline counts for a frame drawing the chat panel at 1, 10
and 100 retained messages, with a body of ~200 bytes and of ~8 KiB; and for a
frame drawing the change review at a small and a 512 KiB patch.

### WP2 — Cache the chat rows

Intent: stop wrapping every body per draw.

Required changes: the cache and its key, per §6; the accessor borrows.

Acceptance evidence: `chat_row_builds` and `chat_wrap_passes` are 0 on a second
frame with an unchanged key and 1 when it changes. The panel's and the box's
rendered buffers are identical.

### WP3 — Prove the redaction and edit directions

Intent: the cache's security property.

Required changes: tests for a body becoming redacted, a body being edited
(`revision` bumped), a channel switch, a project switch, and a session teardown.

Acceptance evidence: each test fails if the cache is keyed without that input.
Demonstrate the redaction case by perturbing the key (dropping `redacted`) and
showing the test fails; record the perturbation.

### WP4 — Remove the unread scan from the frame

Intent: one O(window) filter per draw, whose answer changes rarely.

Required changes: the incremental count or the memo, per §6.

Acceptance evidence: `chat_unread_scans` is 0 on a frame that applied nothing;
the header badge and the unread divider are identical.

### WP5 — Stop cloning the visible rows twice

Intent: the second deep copy of rows that were just allocated.

Required changes: build the viewport rows from the cached rows by borrowing, or
move the rows into the `Paragraph` rather than cloning each `Line`.

Acceptance evidence: no `Line` clone per visible row; identical buffers.

### WP6 — Borrow the review's hunks

Intent: remove up to 512 KiB of copying per frame.

Required changes: the borrowed render path on `DiffViewer`; the review passes
`&patch.hunks`.

Acceptance evidence: `review_hunk_copies` is 0; the review's rendered buffer is
identical in both `DiffMode`s and at several scroll offsets.

### WP7 — Prove the rendering is unchanged, and the counters bite

Intent: the primary acceptance property.

Required changes: a render comparison over a fixture set — an empty channel, one
message, a message that wraps, a message with a multi-byte body, a message with
an escape sequence, a redacted message, the unread divider at the first and last
message, a selected message, a truncated history header, and a diff with
additions, deletions and context. Then perturb one key and confirm the
comparison fails.

Acceptance evidence: identical buffers; a failing comparison under the
perturbation; the perturbation reverted and recorded.

## 8. Failure, cancellation, restart, and contention semantics

- **Failure.** A chat cache that survives a redaction shows content the daemon
  removed. That is the milestone's security failure mode and its stop condition.
- **Cancellation.** A channel switch, project switch or session teardown drops
  the cache. Nothing survives a session.
- **Restart.** Nothing durable; the cache starts empty.
- **Contention.** None; one surface, one thread.
- **A page that arrives mid-frame.** `sequence` bumps when the page is applied,
  so the cache is dropped before the next draw. A page applied *after* the draw
  shows on the next frame, which is the existing behaviour.

## 9. Compatibility and migration

None. No protocol, storage, configuration, or migration change. `chat_browser::rows`'s
signature may change; both call sites are migrated in the same change. No public
API outside `src/tui/`.

## 10. Required tests

### Focused unit tests

- A second draw with an unchanged key performs no row build and no wrap pass.
- A new message (`sequence` bumped) rebuilds.
- A read-marker advance (`last_read_seq` bumped) rebuilds the unread divider and
  not the bodies — or rebuilds both, under key (a); the test asserts the visible
  result, not the internal split.
- A width change rebuilds.
- A cursor move rebuilds the styled rows (key (a)) or costs nothing (key (b)).
- `unread_count` is not a scan on the draw path.
- `ChangeReviewView::render` performs no hunk copy.

### Integration tests

- A frame drawing each chat surface asserts the expected counts, driven through
  `tests/tui_render`.

### Restart and recovery tests

None required.

### Contention and cancellation tests

- A channel switch, project switch and session teardown each drop the cache.

### Security and negative tests

- A body that becomes redacted renders `[redacted]` on the next frame.
- A body that is edited (`revision` bumped) re-renders.
- A channel switch never shows the previous channel's rows.

### Migration and compatibility tests

None.

### Placement and vacuity

`tests/tui_render/tui_render_cost.inc` for the counts; the chat state's own unit
tests for the invalidation and redaction matrix. Every new assertion must be
shown failing against the baseline.

## 11. Required verification commands

```bash
# The chat state's and browser's own tests.
cargo test -p codegg --lib -- tui::app::state::chat tui::chat_browser

# The two surfaces.
cargo test -p codegg --lib -- tui::panels::chat tui::components::dialogs::chat \
    tui::components::change_review tui::components::diff

# The cost member and the family.
cargo test --test tui_render -- tui_render_cost
cargo test --test tui_render

# The shape guard, and the proof it bites.
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_cost.py --self-test

# Rust-aware test inventory.
python3 scripts/rust_test_scan.py --validate

# Scoped, then the full sweep before closure.
scripts/verify.sh scoped HEAD~1
bash scripts/verify.sh full
```

## 12. Documentation updates

- `architecture/tui.md` §"The render path's caches (C006)": the two new rows.
- `architecture/tui.md` §"One chat browser, two surfaces": one producer of the
  rows, both surfaces reading it.
- The closure record must carry the `MAX_CHAT_BODY_DISPLAY_LEN` finding from §5
  as a deferred, product-visible decision — not as a silent omission.

## 13. Acceptance criteria

1. A draw with an unchanged channel, width and cursor performs **zero** message
   body wraps, down from one per retained message per draw.
2. A redacted message renders `[redacted]`, and an edited message re-renders —
   each proven by a test that fails under a perturbed key.
3. The unread count is not recomputed by scanning the window on the draw path.
4. The visible rows are not deep-cloned a second time per draw.
5. The change review performs **zero** hunk copies per frame, down from one
   whole-patch copy.
6. The rendered output is byte-identical to the baseline for the fixture set,
   and the comparison is demonstrated capable of failing.
7. The chat window's retention, paging, and the `selected_row_end` anchor rule
   are unchanged.

## 14. Stop conditions

Stop and report rather than improvise if:

- A cache key cannot be found that covers the redaction and edit directions.
  Serving a pre-redaction body is a security defect.
- Meeting criterion 1 would require applying `MAX_CHAT_BODY_DISPLAY_LEN` on the
  render path. That is a user-visible truncation and belongs to a product
  decision, not to a cost milestone.
- The two chat surfaces would need a shared cache across their two cursors. They
  are separate surfaces with separate browsers; a shared cache would be a second
  ownership boundary. Two caches, or one cache keyed on the surface.
- Borrowing the review's hunks would require changing `DiffViewer`'s public
  fields or its `compute_diff`. The borrowed render path must be additive.
- The change would touch the chat protocol, the reducer's paging, or
  `unified_diff.rs`.
- A redaction test cannot be made to fail under a perturbed key.

## 15. Closure evidence required

- The baseline and after counts for all four counters, per surface, with the
  message counts and body sizes used.
- The invalidation and redaction matrix's results, and the perturbation shown
  failing.
- The render comparison's result and its demonstrated ability to detect a
  difference.
- The `MAX_CHAT_BODY_DISPLAY_LEN` finding, recorded as deferred.
- `cargo test --test tui_render`, the touched modules' unit tests, and
  `scripts/verify.sh quick`; `scoped` and `full` labelled local versus CI.
- The registry update and the unblock audit.

## 16. Handoff notes

- **The amplifier is that the display cap is not on this path.** Read
  `state/chat.rs:58` and `:1573` before deciding the cost is small. It is not:
  the renderer wraps bodies the command-line projection truncates.
- **Do not apply the cap to fix the cost.** That is the trap in this milestone.
  Record it; do not take it.
- **`sequence` is the reducer's own monotonic counter.** Use it; do not add a
  second revision.
- **The cursor is an input to the styled rows**, so key (a) is correct. It is
  not a lazy key — it is the honest one. Key (b) is better only because it moves
  the styling out of the cached value.
- **Two surfaces, two browsers.** The panel's cursor and the box's cursor are
  different. Do not share a cache across them.
- **The review's clone is the whole patch.** `DiffViewer::new` with empty inputs
  is cheap; the assignment is not.
- **`selected_row_end` reverse-scans all rows.** Under key (a) it runs only on a
  miss, which is acceptable; under key (b) it must be replaced by the offset
  index.
