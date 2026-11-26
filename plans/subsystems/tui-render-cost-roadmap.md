# TUI Render Cost Roadmap

Status: closed (2026-11-10) — all six milestones implemented, verified and
closed; each has its own closure record under `plans/closure/tui-render-cost/`.

Repository baseline reviewed: `01c45971d4330cfd1bf6541510de1eeef90b7dbc`
(branch `feat/auto-20261109-f8abad0d`)

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/tui.md` — the authoritative TUI module contract, and the owner
  of "The render path's caches (C006)" and "The transcript's retention"
- `architecture/testing.md` — test families, nextest profiles, and the
  Rust-aware scanner rules this roadmap's evidence depends on

Related ADRs:

- **None required.** No milestone in this roadmap changes a canonical identity,
  an ownership boundary, a protocol, a storage layout, or the
  authentication/authorization model. Every milestone is a **cost** change: it
  memoizes a pure function of a key, or moves a computation off the frame,
  without changing what is drawn.

## 1. Purpose and ownership boundary

This workstream carries a second audit of the TUI's render path, taken at the
baseline above. It is the successor to `repository-audit-corrective` C006
("TUI render-path and transcript cost"), which is **closed** and whose closure
record named four findings it did not discharge.

It owns:

- the per-frame cost the **frame composition** pays around its content — the
  retained frame buffer, the sidebar projection, the sidebar's own line model,
  the subagent tab's clone, the composer hint line, the status bar, and the
  tasks overlay's rows;
- the **row models** of the surfaces that rebuild a whole listing per frame:
  the file tree (`/files` and the `Ctrl+T` pane), the project chat, and the
  change review;
- the **rebuild cost** of the transcript: the streaming tail's assembly, a
  message part's rows, a collapsed tool call's structured outline, and the
  code-block wrap;
- the **editor's draw path**: the per-visible-row line read, the semantic-token
  scan, the per-segment style lookup, and the highlight cache's prefix re-hash;
- the **internal cost** of the two shared wrap engines, under the contract
  constraints the conditionally-closed text-rendering lines impose.

It consumes, and does not redefine:

- the frame cadence, the spinner table, `ActivityPhase`, and the render's
  `catch_unwind` — all owned by `architecture/tui.md` and **not** in scope;
- the wrap primitives' contracts and visual-row geometry, owned by
  `tui-main-buffer-text-rendering` and `tui-composer-text-layout`, both
  **conditionally closed** with named conditions;
- the transcript's retention decision, recorded by C006 in
  `architecture/tui.md` §"The transcript's retention" — this roadmap measures
  the cost of that decision and does not reverse it;
- the disclosure and collapse model, the key-ownership model, the command
  catalogue, and the projection contract.

The governing rule, carried over from the audit that produced C006:

> A cost change is admissible only when the rendered output is provably
> unchanged and the cache has an invalidation that is a function of what the
> cached value depends on. A cache with no correct invalidation is a
> correctness defect wearing a performance hat.

## 2. Work classification

### Invariants

- **The rendered output is unchanged.** Every milestone's primary acceptance
  property is byte-identical rendering for a given state, proven over a fixture
  set, with the comparison shown capable of detecting a difference.
- **The frame cadence is unchanged.** One spinner cadence, one `ActivityPhase`
  set, one 16 ms streaming interval, one `catch_unwind` boundary.
- **One producer per derived value.** The transcript's row count still comes
  from the layout cache alone; the composer's rows still come from one wrap;
  a milestone that introduced a second producer of either would reintroduce
  the defect the disclosure work removed.
- **Every cache is invalidated by what its value depends on, and nothing else.**
  A cache keyed coarser discards work it could have kept; a cache keyed finer
  keeps work it must not.
- **The cost counters stay honest.** `src/tui/render_cost.rs` counts
  operations, not time, and a new counter must have a producer in the render
  path and an assertion in `tests/tui_render/tui_render_cost.inc`.

### Capabilities

None. This workstream is not user-visible. No surface gains, loses, or changes
a feature. That is a deliberate boundary, not a shortfall: C006 recorded the
same, and the one place the two workstreams could have become user-visible —
the transcript's retention — was decided against.

### Infrastructure

- A per-frame cost counter set that makes each milestone's claim enforceable
  rather than asserted once in a closure record.

### Polish

Every milestone. This is a cost workstream and is sequenced as polish by
construction, after the correctness boundaries it touches have closed.

## 3. Non-goals

- **The event loop.** C006's audit found it sound and this audit re-confirmed
  it. Nothing in `src/tui/runtime/` changes.
- **A benchmark harness.** No wall-clock claim is made anywhere in this
  workstream. The evidence is deterministic operation counts. Creating a
  timing harness is a separate decision with its own justification problem on
  a shared machine.
- **The transcript's retention.** The unbounded policy and the layout cache's
  second copy are C006's recorded decision. Reversing it would be a
  user-visible change and therefore a different milestone.
- **Fixing the two recorded defects of the conditionally-closed text-rendering
  lines** — the transcript's search-highlight and search-scroll defects, and
  the plain `wrap_to_strings` engine's lack of escape-atomicity on the
  tool-output path. This roadmap must not fix them, must not move them, and
  must not make them harder to fix.
- **The daemon, the protocol, the store, and the projection.** Every cost here
  is frontend-side.
- **Any change to what a surface shows**: the number of rows a listing draws,
  the number of tasks the overlay names, the number of transcript rows a
  session retains.
- **The desktop frontend.** `apps/desktop/` has its own renderer and its own
  qualification lane; nothing here transfers by assumption.

## 4. Current state

The render path's **caches** are in good shape and this roadmap does not
re-derive them. `components/prompt.rs` caches the composer's wrapped layout
against the buffer's revision and the wrap width; `components/messages.rs`
caches a message part's Markdown against `(message, part, length, width)` and
parses the streaming tail block by block; `editor_highlight.rs` keys the
editor's spans on the **content** of each scanned line rather than the
document's revision. `set_theme`, `set_width` and `set_display_options` all
guard their invalidation, so none of them thrashes.

The render path's **callers** are where the remaining cost sits. Five shapes
recur, and every milestone below is one of them:

1. **A whole-surface clone made for a reader that does not run.** The frame's
   `finish_render` retains a deep copy of the terminal buffer on every frame,
   and the only reader returns early unless a text selection exists.
2. **A projection re-materialized per frame, per surface.** The sidebar's
   population step deep-clones the projection snapshot — jobs, agent runs, run
   groups, the agent tree, convergences, tool programs, subagents, every
   changed file's diff preview, git state — inside the per-panel render loop,
   and applies its row bounds *after* the clone.
3. **A row model rebuilt per frame with no cache at all.** The project chat
   wraps every retained message body on every draw; `/files` re-filters and
   re-clones the whole listing three to four times per frame; the `Ctrl+T`
   tree pane flattens the whole tree per frame; the change review deep-clones
   the parsed diff per frame.
4. **A cached value re-copied on every hit.** A message part's cached rows are
   cloned twice per rebuild; the streaming tail re-assembles (and therefore
   re-clones) its whole block set on the fast path where nothing changed.
5. **A per-row or per-character cost where a per-viewport one would do.** The
   code-block wrapper emits one `Span` and one heap `String` per character;
   the editor reads each visible row whole and walks it past the column limit;
   the semantic-token scan is linear over all tokens for every row; the styled
   wrap rescans its span list from the front for every word.

### Why the frequency matters

`src/tui/runtime/event_loop.rs` forces a redraw on every pump tick while
streaming, while a turn is `Working`, or while a toast is showing. Combined
with the 16 ms streaming interval and the 80 ms spinner interval, a full frame
runs at roughly **62 Hz during streaming and 12.5 Hz while a turn is merely
working**. Every shape above is therefore paid 12.5–62 times per second, and
the spinner alone pays for all of it whether or not anything changed.

That is also why this workstream must not "fix" the cadence: the cadence is
the contract, so the cost has to leave the frame instead.

### The carried-over findings from C006

`plans/closure/repository-audit-corrective/006-status.md` §12 names four
findings it did not discharge. This roadmap owns three of them and records the
fourth as closed:

| C006 §12 finding | Disposition here |
|---|---|
| The file tree's row list is still rebuilt and re-cloned two to three times per frame, and its `selected_index` lookup rebuilds the list | **Owned by M002, and discharged.** C006 stopped because the rows carry paths and need an invalidation proven against a stale path. M002 made that proof its first acceptance criterion and delivered the matrix: removed, renamed, collapsed, re-scoped, replaced-listing, project-switch and teardown each have a test, and dropping `revision` from the cache key makes all of them fail. The finding can leave this list. |
| The `/search` path still clones every message part | **Not owned.** Deliberate: the search path is where the two recorded defects of the conditionally-closed text-rendering lines live. A cost change there would have to touch the code those defects are in. |
| The plain `wrap_to_strings` engine still lacks escape-atomicity on the tool-output path | **Not owned, and must not be fixed here.** M006 changes that engine's internal cost only; escape-atomicity is a behavioural change owned by the text-rendering line. |
| The tasks overlay's construction is still proportional to the total task count, because each group header's "N more" needs the group's total | **Owned by M001**, as a narrower claim: the overlay must be built **once** per frame instead of twice, and its per-subagent linear lookup must stop allocating. The proportionality itself is accepted, with the reason recorded. |

## 5. Target architecture

One rule, applied six times:

> **A frame reads cached derived state and prepares widgets. It derives
> nothing that a previous frame already derived from the same inputs.**

Concretely, after this workstream:

- A frame with an unchanged state performs **no** deep clone of the terminal
  buffer, **no** projection re-materialization, and **no** row-model rebuild
  for any list surface it draws.
- A surface that must retain state for a *later* interaction (the frame buffer
  for a drag, the change review's hunks) retains it **only while that
  interaction is live**.
- Every cache introduced here is keyed on a value the surface already owns —
  a projection revision, a listing revision plus filter, a message window plus
  body width, a document revision plus visible range — and each key is
  demonstrated by a test that changes the key and asserts the rebuild, and
  changes something else and asserts the absence of a rebuild.
- The cost counters in `src/tui/render_cost.rs` grow to cover the shapes above,
  so a regression is caught by `scripts/check_tui_render_cost.py` or by the
  `tui_render` family rather than by a user noticing.

## 6. Dependency graph

All six milestones are **mutually independent**. They touch disjoint file
sets, and none consumes another's output. They may be executed in any order,
on separate branches, from the same baseline.

| Milestone | Hard dependencies | Interface dependencies | Conflict risk |
|---|---|---|---|
| M001 frame composition | none | `architecture/tui.md` frame contracts (read-only) | `src/tui/app/render.rs` is also touched by M002 and M003 at distinct functions; a single-branch execution must serialize the file, not the milestones |
| M002 file tree row model | none | `FileTreeStructure`'s revision contract | `src/tui/app/render.rs` (`refresh_file_tree_surfaces`) shared with M001 |
| M003 chat and change review | none | `ChatView`'s message window and `revision`; `DiffHunk`'s shape | `src/tui/app/render.rs` (`render_change_review`) shared with M001 |
| M004 transcript rebuild | none | the one-producer rule for the transcript's rows | `src/tui/components/messages.rs` only |
| M005 editor draw path | none | `DocumentSnapshot`'s read API; the highlight cache's content-keying | `src/tui/components/editor.rs`, `editor_highlight.rs`, `text_geometry.rs` only |
| M006 shared wrap engine | none | the wrap contracts of both conditionally-closed lines | `src/tui/wrap.rs` only |

Classification: every edge above is a **soft** dependency (a shared file, not
a semantic prerequisite). There is no hard dependency and no interface contract
that must stabilise first.

**Sequencing by consequence, not by dependency** (the same rule the audit
workstream used): M001 first, because it is the largest unconditional per-frame
cost and the smallest change; M002 second, because it discharges a named C006
debt and its stale-path proof is the one piece of this workstream that could
legitimately fail; M003 third, because the chat surface is the one place the
cost scales with total history rather than with the viewport; M004 and M005
fourth and fifth, because their cost is bounded by content size and the
viewport respectively; M006 last, because its benefit is now per-content-change
rather than per-frame and its contract constraints are the tightest.

## 7. Milestones

### M001 — The frame's own per-frame work

- **Class:** polish (cost).
- **Objective:** a drawn frame performs no deep clone of the terminal buffer it
  does not read, materializes the sidebar's projection and line model at most
  once, and builds the tasks overlay's rows once.
- **Deliverable boundary:** `src/tui/app/render.rs`,
  `src/tui/components/sidebar.rs`, `src/tui/components/status_bar.rs`, and the
  `App` methods the frame calls from `src/tui/app/mod.rs`.
- **Exit conditions:** the retained-buffer clone happens only while a selection
  is live; the sidebar's projection step runs at most once per frame and only
  when the projection moved; the sidebar's line model is built once per frame;
  the subagent view borrows its tab instead of cloning it; the composer hint
  line is built once; the tasks overlay's rows are built once.
- **Plan:** `plans/implementation/tui-render-cost/001-frame-composition-cost.md`

### M002 — The file tree's row model

- **Class:** polish (cost).
- **Objective:** the file tree flattens and filters its listing at most once per
  frame, behind a cache whose invalidation is proven against a stale path.
- **Deliverable boundary:** `src/tui/app/state/file_tree.rs`,
  `src/tui/components/file_tree.rs`, `src/tui/components/dialogs/files.rs`.
- **Exit conditions:** `/files` and the `Ctrl+T` pane each read one row slice
  per frame; the selection lookup does not rebuild the list; a structure change
  or a filter change invalidates the cache and a test proves both directions,
  including that a removed path is never displayed.
- **Plan:** `plans/implementation/tui-render-cost/002-file-tree-row-model.md`

### M003 — The chat and change-review row models

- **Class:** polish (cost).
- **Objective:** the project chat wraps each message body once per content
  change rather than once per draw, and the change review borrows its parsed
  diff instead of cloning it.
- **Deliverable boundary:** `src/tui/chat_browser.rs`,
  `src/tui/panels/chat.rs`, `src/tui/components/dialogs/chat.rs`,
  `src/tui/components/change_review.rs`.
- **Exit conditions:** a draw with an unchanged channel and width performs no
  body wrapping; the viewport is derived from cached offsets rather than from a
  scan of all rows; the change review's hunks are borrowed.
- **Plan:** `plans/implementation/tui-render-cost/003-chat-and-change-review-row-models.md`

### M004 — The transcript's rebuild cost

- **Class:** polish (cost).
- **Objective:** a streaming tick copies the tail's rows once instead of twice,
  a rebuilt message copies its parts' cached rows once instead of twice, a
  collapsed tool call does not parse its payload, and the code-block wrapper
  emits one span per style run rather than one per character.
- **Deliverable boundary:** `src/tui/components/messages.rs`.
- **Exit conditions:** the tail's assembly is not repeated on the unchanged
  fast path; `markdown_part_rows` hands back a borrow or the caller consumes it
  without a second copy; the structured outline is built only under the
  `expanded` gate; the code-block wrapper's span count is bounded by style runs.
- **Plan:** `plans/implementation/tui-render-cost/004-transcript-rebuild-cost.md`

### M005 — The editor's draw path

- **Class:** polish (cost).
- **Objective:** a visible editor row is read and walked only as far as the
  column window needs, the semantic-token and syntax-style lookups are bounded
  by the row, and the highlight cache's prefix check stops re-reading the whole
  prefix on every keystroke.
- **Deliverable boundary:** `src/tui/components/editor.rs`,
  `src/tui/editor_highlight.rs`, `src/tui/text_geometry.rs`.
- **Exit conditions:** a visible row's cost is proportional to the drawn
  columns; the token scan is `O(log n + tokens_on_row)`; the style lookup is a
  single sweep; the prefix check starts from the edit; the cache-key comparison
  allocates nothing on a hit.
- **Plan:** `plans/implementation/tui-render-cost/005-editor-draw-path-cost.md`

### M006 — The shared wrap engines' internal cost

- **Class:** polish (cost).
- **Objective:** the styled wrap stops rescanning its span list from the front
  for every word and stops measuring a word twice, without changing its output,
  its signature, or its escape handling.
- **Deliverable boundary:** `src/tui/wrap.rs` — `build_items`, `split_at_spans`,
  `style_at`, and the plain engine's per-word work.
- **Exit conditions:** the styled engine's span work is linear in
  `spans + words`; each word is measured once; the rendered rows and the
  byte offsets are identical for the fixture set that includes a CSI sequence
  and an OSC-8 hyperlink; no signature changed and no recorded defect moved.
- **Plan:** `plans/implementation/tui-render-cost/006-shared-wrap-engine-cost.md`

## 8. Cross-cutting requirements

**Storage.** None. No migration, no schema, no durable state.

**Protocol.** None. No DTO, no wire variant, no daemon change.

**Security.** Two consequences to check rather than assume, both already named
by C006 and both live here:

- M002's cache holds **file paths**. It must be invalidated when the listing or
  the filter changes, it must not outlive the tree it describes, and it must
  not retain a path from another session's listing.
- M003's chat cache holds **message bodies**, including redacted ones. A
  redacted body must stay redacted through the cache; the cache must key on the
  message identity and revision so a body that becomes redacted is re-rendered
  rather than served from a pre-redaction entry.

No authorization boundary changes. No cache in this workstream is shared across
sessions, and none survives a session teardown.

**Concurrency.** Every cache here is frontend-side, per-widget, and touched only
from the render thread or the input thread that owns the widget. None is shared
across threads, so none needs a lock. A cache that would need one is a signal
that the value belongs somewhere else — a stop condition, not an invitation.

**Observability.** `src/tui/render_cost.rs` gains counters for the shapes this
workstream removes, so the claims are enforceable. A counter with no producer
in the render path is a claim, not a hook.

**Docs.** `architecture/tui.md` §"The render path's caches (C006)" grows a
table row per new cache, keyed on what the value depends on and dropped by what.
The section already states the rule; this workstream adds the entries.

## 9. Verification strategy

Three layers, in this order, because each one is cheaper than the next:

1. **Operation counts.** `tests/tui_render/tui_render_cost.inc` asserts *how
   many* wraps, parses, scans, copies and rebuilds a frame performs. A new
   counter is added with its producer and its assertion in the same change.
2. **Render equivalence.** A warm-cache render compared against a cold one over
   a fixture set, with the comparison demonstrated to fail when a cache key is
   deliberately coarsened. `tests/tui_render/tui_render_cost.inc` already holds
   the shape for the composer and the streaming tail; M003, M004 and M006 extend
   the fixture set to the chat body, the code block, and the styled wrap.
3. **Shape guards.** `scripts/check_tui_render_cost.py` pins the shape and has
   a `--self-test` that injects each violation class. A new invariant that
   fails *silently* belongs here rather than in a test somebody has to
   remember to run.

`cargo test --test tui_render` is the family. New members are
`tests/tui_render/<name>.inc` bound in `tests/tui_render/mod.rs` with
`#[path = "<name>.inc"] pub mod <name>;`.

**No wall-clock claim is made in any milestone of this roadmap**, and a closure
record that makes one without a harness is a closure defect.

## 10. Risks and decision points

- **R1 — A cache with no correct invalidation.** The dominant risk, and the
  reason each milestone's first acceptance criterion is the invalidation proof
  rather than the speedup. Mitigation: every plan names its stop condition as
  "the invalidation cannot be proven".
- **R2 — Reopening a conditionally-closed line.** `tui-main-buffer-text-rendering`
  and `tui-composer-text-layout` are conditionally closed with named
  conditions. M006 touches their file. Mitigation: M006 may change only
  internal cost, never a signature, an output, an escape rule, or the search
  path; its plan makes each of those a stop condition.
- **R3 — Retaining state for an interaction that is not live.** The frame
  buffer and the change review's hunks are both retained for a *later* user
  action. The fix must retain them *while the action is possible*, not "always"
  and not "never". A change that makes a drag or a review render stale content
  is a regression, not an optimisation.
- **R4 — The bound is applied after the clone.** Several sites apply their row
  bound after materializing the whole set. Moving the bound earlier is safe
  only where the discarded tail is genuinely unused — a group total, for
  instance, needs the whole group. Each plan states which bounds may move and
  which may not.
- **R5 — Measurement theatre.** A counter that is never asserted, or an
  assertion that cannot fail, is worse than no counter. Mitigation: every new
  assertion is demonstrated failing against the pre-change code, the way C006's
  baseline capture did.

**Decision points.** None is open. No ADR is required, no product decision is
implied, and no milestone depends on an owner choice. The one decision that
*would* have been open — the transcript's retention — was taken by C006 and is
recorded rather than revisited.

## 11. Completion definition

This roadmap is complete when:

1. All six milestones are `closed`, each with its own closure record under
   `plans/closure/tui-render-cost/`.
2. Every counter this workstream adds has a producer and an assertion, and each
   assertion has been shown failing against the code before its change.
3. `scripts/check_tui_render_cost.py` covers the shapes that would fail
   silently, and its `--self-test` rejects each new violation class.
4. `architecture/tui.md` §"The render path's caches" names every cache this
   workstream introduced, with its key and its invalidation.
5. A frame with an unchanged state performs no deep clone of the terminal
   buffer, no projection re-materialization, and no row-model rebuild for the
   surfaces it draws — asserted, not asserted-in-prose.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 frame composition | **closed** | `plans/implementation/tui-render-cost/001-frame-composition-cost.md` | `plans/closure/tui-render-cost/001-status.md` | none; WP6 (status-bar summary) deferred under the plan's §14 |
| M002 file tree row model | **closed** | `plans/implementation/tui-render-cost/002-file-tree-row-model.md` | `plans/closure/tui-render-cost/002-status.md` | none; discharges C006 §12's file-tree finding |
| M003 chat and change review | **closed** | `plans/implementation/tui-render-cost/003-chat-and-change-review-row-models.md` | `plans/closure/tui-render-cost/003-status.md` | none; `MAX_CHAT_BODY_DISPLAY_LEN` deferred to a product decision |
| M004 transcript rebuild | **closed** | `plans/implementation/tui-render-cost/004-transcript-rebuild-cost.md` | `plans/closure/tui-render-cost/004-status.md` | none; `render_md_line_wrapped` deferred (would change output) |
| M005 editor draw path | **closed** | `plans/implementation/tui-render-cost/005-editor-draw-path-cost.md` | `plans/closure/tui-render-cost/005-status.md` | none; ADR-0011 held with no allowlist change |
| M006 shared wrap engine | **closed** | `plans/implementation/tui-render-cost/006-shared-wrap-engine-cost.md` | `plans/closure/tui-render-cost/006-status.md` | none; the roadmap's deferrable milestone, landed |

## 13. Closure summary

The roadmap's §11 completion definition is met:

1. All six milestones are `closed`, each with its closure record.
2. Every counter this workstream added has a producer on the render path and an
   assertion in the `tui_render` family, and each assertion was shown failing
   against the code before its change.
3. `scripts/check_tui_render_cost.py` grew from 7 checks and 7 injections to **28**
   checks and **35** injections, each injection proving its check is not vacuous.
4. `architecture/tui.md` §"The render path's caches (C006)" names every cache
   this workstream introduced, with its key and its invalidation.
5. A frame with an unchanged state performs no deep clone of the terminal buffer,
   no projection re-materialization, and no row-model rebuild for any list surface
   it draws — asserted by operation counts, not by prose.

Four items are recorded as deferred rather than silently omitted: the status-bar
summary cache (M001, its key would be as wide as the derivation), the chat body
display cap (M003, a product-visible truncation), `render_md_line_wrapped` (M004,
routing it would change rendered output), and the plain wrap engine's
escape-atomicity defect (M006, owned by the text-rendering line).

Two cross-milestone items were fixed at this closure rather than inside a
milestone, because both are properties of the tree: `docs/comment-budget.toml`
was banked (the guard was red with 54 R4 violations, 39 of them from M001–M004),
and a `field_reassign_with_default` warning in M003's test was fixed so that
`clippy -D warnings` is clean.

### The corrective pass of 2026-11-10

A review of the pre-existing defects and residual risks this roadmap recorded
rather than fixed produced five fixes and one guard hardening. They are recorded
in the affected milestones' closure records, not as a new milestone, because
three of them are one-line corrections to code this roadmap already touched and
two are test-or-guard work.

- **The streamed table** (M004): `reparse_start_line` now backs up one block,
  because a block's meaning can depend on the block before it. The defect test
  became a positive test and the table joined `INCREMENTAL_FIXTURES`.
- **The suppressed reasoning header** (M004): the search arm's
  `prev_was_reasoning` reset is unconditional.
- **The wrap's trailing-run double count** (M006): `chunk_word` no longer adds a
  run's width twice, so a multi-span word breaks at its true width. This changes
  where rows break, so it is the one fix that is a visible rendering change.
- **The `(rN)` marker** (M003): the body wraps at the width minus the marker and
  the selection padding reserves the marker's columns, so an edited message
  cannot look unedited at sidebar width. The owner chose the narrowing trade-off.
- **The file tree's `pub` row inputs** (M002): a new
  `file-tree-row-input-write` guard rejects a direct write through
  `file_tree_state.` from outside the authority, and the retargeted
  `tasks-whole-list-clone` injection is a genuine deep clone again.
- **Coverage**: `a_message_parts_markdown_is_parsed_once` now really builds five
  parts, and the composer-hint test covers resize mode.

Two items were reported as defects and are **not** defects: the two plan
"links" a link-check flags in `plans/registry.md` are a `../gorouter`
sibling-repository path and a filename **pattern** (`ADR-NNNN-….md`) in prose.

### The tree-level verification, recorded once

Each milestone's closure record points here for the routine subset, because it is
a property of the whole roadmap rather than of one milestone.

```text
$ bash scripts/verify.sh quick
... 145 guard and step sections ...
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3m 25s
==> Quick verification passed.
EXIT=0
```

That is the canonical sanity pass: rustfmt, the two generated-asset checks, the
`scripts/check_*` guard set (**86** distinct scripts ran, each with its self-test),
and a workspace `cargo check --all-targets --locked`. The
`FAIL:` lines that appear in its output are `--self-test` injections — each is
followed by a `[PASS] self-test: … rejected` line and the guard then passes on the
real tree. Reading them as failures would be a misreading of the guard's own
protocol.

Beyond `quick`, at this closure:

- `cargo test --test tui_render` — **1189 passed, 0 failed**, the family with all
  six cost members bound.
- `cargo clippy -p codegg --lib --tests -- -D warnings` — clean.
- `python3 scripts/check_tui_render_cost.py` and `--self-test` — passes, **35**
  injections all rejected.
- `python3 scripts/check_comment_budget.py` and `--self-test` — passes.
- `python3 scripts/rust_test_scan.py --validate` — all checks pass.
- The five editor guards — all green with no allowlist change.
- `git diff --stat -- src/tui/runtime` — empty; `git diff --cached --stat` — empty.

`scripts/verify.sh full` and `scripts/verify.sh scoped` were **not** run: `full`
adds the nextest workspace sweep and the bounded feature pass, which is the
hosted lane's job, and `scoped` widens to every transitive dependent's surface for
a root-package change — C006 recorded that it did not finish inside one
milestone's budget, and the same holds here. Neither is claimed as passing.
