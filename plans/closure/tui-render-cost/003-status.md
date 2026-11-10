# TUI Render Cost Milestone 003 — Closure Status

Status: **closed** (2026-11-10), with one recorded deferral that is a product
decision (§8).

Implementation plan:
`plans/implementation/tui-render-cost/003-chat-and-change-review-row-models.md`
Source roadmap: `plans/subsystems/tui-render-cost-roadmap.md` §7 (M003)
Repository baseline: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
No ADR was required, and none was created.

## 1. Executive finding

The project chat had **no cache anywhere**. `chat_browser::rows` flattened the
whole retained window — up to `MAX_CHAT_MESSAGES_PER_CHANNEL = 100` messages —
into fresh `ChatRow`s whose `Line<'static>` fields force every wrapped chunk to be
an owned `String`, and both surfaces called it on every draw. The cost was
proportional to **total retained history × body size**, not to the viewport.

The amplifier is that the chat surface's own display cap is **not** on this path:
`MAX_CHAT_BODY_DISPLAY_LEN = 500` is enforced only in `message_line`, the
command-line projection. So the renderer wrapped bodies the command line
truncates, at sidebar width (~28 columns), producing hundreds of rows per message.

Separately, the change review deep-cloned the whole parsed patch on every frame
the review was open — bounded by `MAX_REVIEW_PATCH_BYTES = 512 KiB` — to hand it
to a viewer that only ever reads it.

## 2. The cost sites, before and after

| Surface | Retained msgs / body | Before, per frame | After, unchanged frame | After, one change |
|---|---|---|---|---|
| docked panel | 1 / 200 B | 1 build, 1 wrap, 1 unread scan | **0, 0, 0** | 1, 1, 0 |
| docked panel | 10 / 200 B | 1, 10, 1 | **0, 0, 0** | 1, 10, 0 |
| docked panel | 100 / 200 B | 1, 100, 1 | **0, 0, 0** | 1, 100, 0 |
| docked panel | 100 / 8 KiB | 1, 100, 1 | **0, 0, 0** | 1, 100, 0 |
| `/chat` box | 1 / 10 / 100 | 1, N, 0 | **0, 0, 0** | 1, N, 0 |
| change review | small patch (2 hunks) | 2 hunk copies | **0** | — |
| change review | 512 KiB patch (237 hunks) | 237 hunk copies | **0** | — |

The box's `unread` arrives inside the pushed snapshot, so its draw path never
scanned — before or after. That is reported rather than papered over.

### 2.1 The captured baseline failures

Captured by forcing the pre-change operation sequence (cache always miss, memo
disabled, review clone restored) with the counters in place:

```
left: 1, right: 0   "the panel rebuilt its row model on a frame where nothing changed"
left: 10, right: 0  "and wraps nothing"
left: 1, right: 0   "the at the bound review copied its parsed patch on a frame"
```

## 3. The shape

- `ChatBrowser` owns a `ChatRowCache` keyed on
  `(sequence, unread, channel_id, width, cursor, theme)` — six inputs, written as
  an explicit comparison so the channel is compared as `&str` (the view borrows
  it) and so every input is named in one place. `rows` is the single producer;
  `window_lines` hands back the viewport window by borrow.
- **Two surfaces, two caches.** The docked panel and the `/chat` box have separate
  browsers, separate cursors and separate caches.
- `ProjectChat::unread_count` is memoized on
  `(sequence, last_read_seq, messages.len())` through a `Cell`. The `messages.len()`
  half is there because `set_capability(false)` clears the window **without**
  moving `sequence`.
- `DiffViewer` gained `render_hunks_in(&self, hunks: &[DiffHunk], area, buf, theme)`
  with `render_in` delegating to it with `&self.hunks`. `ChangeReviewView::render`
  passes `&patch.hunks`.

## 4. The invalidation and redaction matrix — the security property

| Source | Tests | Result |
|---|---|---|
| new message | `a_new_message_rebuilds_the_rows_once`, `a_new_message_rebuilds_the_panel_once` | pass |
| read-marker advance | `a_read_marker_advance_rebuilds_the_unread_divider`, `…_the_panel`, `the_unread_count_is_not_recomputed_by_a_draw_that_changed_nothing` | pass |
| width change | `a_width_change_rebuilds_and_the_same_width_does_not`, `…_the_panel_…` | pass |
| cursor move | `a_cursor_move_rebuilds_the_styled_rows`, `a_cursor_move_rebuilds_the_panel_rows` | pass |
| theme change | `a_theme_change_rebuilds_the_rows` | pass |
| **redaction** | `a_body_that_becomes_redacted_is_re_rendered_on_the_next_read`, `a_redaction_reaches_the_next_frame`, `a_warm_panel_draws_what_a_cold_app_draws_after_a_redaction` | pass |
| **edit** (`revision` bumped) | `an_edited_body_is_re_rendered_on_the_next_read`, `an_edit_reaches_the_next_frame` | pass |
| channel switch | `another_channels_rows_are_never_served_from_the_cache`, `a_channel_switch_drops_the_panel_rows` | pass |
| project switch | `a_project_switch_drops_the_panel_rows` | pass |
| session teardown | `a_reset_drops_the_cached_rows`, `a_session_teardown_drops_the_panel_rows` | pass |

**The perturbation, recorded and reverted.** Dropping `self.sequence ==
request.sequence` from `ChatRowsKey::covers` made the redaction test fail with

```
a redacted body was served from the pre-redaction rows:
" alice first bob second▍alice third                            "
```

and the edit test fail, and the frame-level
`a_warm_panel_draws_what_a_cold_app_draws_after_a_redaction` fail with
`the warm frame after a redaction differs from a cold app in the same state`.
Reverted; all three pass.

**A vacuity defect found and fixed during the milestone.** The first redaction
tests built a *fresh* browser for the second read, so they never exercised the key
at all — they would have passed with no cache. They were rewritten to read through
one browser, and only then did the perturbation bite. This is recorded because a
redaction test that cannot fail is worse than none.

## 5. Invariant review

- **The rendered output is byte-identical.** Chat rows, wrapping, indentation, the
  unread divider's position, the selection marker and the retention header are
  unchanged; the cursor still highlights the whole message; `selected_row_end`'s
  anchor rule is unchanged. Warm/cold buffer comparison over the fixture set:
  empty channel, one message, a wrapping message, a multi-byte body, an escape
  sequence, a redacted message, the divider at the first and last message, a
  selected message, a truncated history header, and a diff with additions,
  deletions and context.
- **A redacted body stays redacted**; an edited body re-renders.
- **One producer of the rows.** The guard forbids both surfaces from calling
  `build_rows`, `wrap_to_strings` or `selected_row_end` while drawing.
- **The chat window's retention is unchanged** (100 messages), as is paging.

## 6. Security review

The cache holds **message bodies, including redacted ones**, so the redaction and
edit directions are the milestone's security property and both are proved by a
test that fails under a perturbed key (§4). The cache is per-project and
per-channel, is dropped on channel switch, project switch and session teardown,
and cannot serve one channel's rows to another. No authorization boundary change;
nothing is shared across threads.

## 7. Migration and compatibility review

None outside `src/tui/`. No protocol, storage, configuration or migration change;
no daemon change. `chat_browser::rows`'s shape changed and both call sites were
migrated in the same change. `ChatMessageDto` already carried `revision` and
`redacted`, so no DTO changed.

Two implementation notes a reader should know:

- **`ChangeReviewView::render` still constructs a blank `DiffViewer`** for its
  mode, scroll and title, rather than not constructing one at all as the plan's
  §6 wording says. `render_hunks_in` is a method needing `&self`, and the plan's
  own §3.4 already establishes that constructing a viewer with empty inputs is
  cheap. What is gone is the viewer **owning** a copy of the hunks. Cost-neutral.
- **`review_hunk_copies`'s producer is attached to `DiffHunk::Clone`**, not to a
  call site: the review's per-frame copy is gone, so a call-site producer would be
  unreachable and would warn. Instrumenting the type means any reintroduced
  whole-patch clone is counted by construction, at the cost of a counter bump in a
  `Clone` impl.

## 8. Unresolved findings and deferrals

- **`MAX_CHAT_BODY_DISPLAY_LEN` on the render path — deferred to a product
  decision, not taken.** The chat render path wraps each retained body in full;
  the 500-byte cap is enforced only in the command-line projection. Applying it to
  the render path would cut the panel's build cost roughly in proportion to body
  size, but it is a **user-visible truncation**: a long message would stop
  mid-sentence in the panel with no affordance for the rest. It is no longer
  urgent, because the cost is now paid once per content change instead of once per
  frame. This is the milestone's named trap and it was not taken.
- **The viewport window is materialized once per draw.** `ratatui`'s `Paragraph`
  needs an owned `Vec<Line>`, so a draw copies the visible rows once. That is
  bounded by the viewport and is not the defect; what was removed is the *second*
  copy of rows that had just been freshly allocated by a full rebuild.
- **The unread count uses the memo, not the incremental form** (the plan permits
  either). A single choke point cannot miss an apply path the way a
  hand-maintained field can.
- **The `(rN)` marker's clipping is fixed** in the corrective pass of
  2026-11-10. The marker was appended after the row's full-width selection
  padding, so at a sidebar-sized panel it fell outside the drawn columns and an
  edited message looked unedited. Two halves were needed: the body wraps at the
  width **minus** the marker, and the selection padding reserves the marker's
  columns on the row it lands on. The trade-off is the owner's choice — every row
  of a revised message is narrower than the same message unrevised — and it is
  recorded at `chat_browser::build_rows`.

## 9. Verification executed

```text
$ cargo fmt --check
exit=0
$ cargo test -p codegg --lib -- tui::app::state::chat tui::chat_browser
test result: ok. 67 passed; 0 failed; 0 ignored; 0 measured; 7795 filtered out
$ cargo test -p codegg --lib -- tui::panels::chat tui::components::dialogs::chat \
    tui::components::change_review tui::components::diff
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 7831 filtered out
$ cargo test --test tui_render -- tui_chat_cost
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 1138 filtered out
$ cargo test --test tui_render
test result: ok. 1159 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
$ python3 scripts/check_tui_render_cost.py
check_tui_render_cost: the render path derives each result once
$ python3 scripts/check_tui_render_cost.py --self-test
self-test: 19 injections, all rejected
$ python3 scripts/rust_test_scan.py --validate
rust_test_scan --validate: all checks pass
```

The family count is the count at this milestone's closure. The tree-level
`scripts/verify.sh quick` result is recorded once, in the roadmap's closure.

## 10. Roadmap disposition and registry update

M003 is **closed**. It discharges nothing from
`plans/closure/repository-audit-corrective/006-status.md` §12 — the `/search`
path's clones remain **not owned**, by the roadmap's deliberate decision, because
that path is where the two recorded defects of the conditionally-closed
text-rendering lines live.

The roadmap's milestone table records M003 as `closed` with this record linked;
`plans/registry.md` moves its row to the closed disposition. The unblock audit
found nothing to unblock.
