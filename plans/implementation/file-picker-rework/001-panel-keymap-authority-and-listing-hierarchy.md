# File Picker Rework Milestone M001 — Panel keymap authority and listing hierarchy

Status: ready for handoff

> **Implemented.** Outcome, verification and the two places this plan was wrong
> are recorded in `plans/closure/file-picker-rework/001-status.md` — read that
> first. §6.4's `!panel_is_empty()` gate was **not** taken literally (it would
> have failed this plan's own §10 requirement); §3's "a scoped listing's rows
> arrive at `depth == 0`" is **factually wrong** — they arrive at the same depth
> the workspace root's top level does, which is the same defect. The body below
> is left as written, as the contract it was approved against.

Repository baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c` (`feat(tui): editor cursor, readable colours, in-file / search, syntax colours`, 2026-10-16)

Source roadmap:

- `plans/subsystems/file-picker-rework-roadmap.md` — §5 target architecture, §7 M001

Long-term requirements:

- `plans/000-long-term-specification.md` — TUI-first product; correctness before transparent magic.
- `plans/001-terminology-and-domain-model.md` — workspace-relative path identity, document attachment.
- `plans/002-long-term-roadmap.md` — TUI evolution on frontend-neutral projections.

Applicable ADRs:

- ADR-0022 (daemon-authorized workspace directory listing) — consumed unchanged.
- ADR-0033 (directory-tied project identity) — consumed unchanged.

Primary class: capability

## 1. Objective

Make the workspace file picker usable and intuitive by (a) giving the panel status line one authoritative copy of the picker's keymap that is always visible while the panel is docked, with no duplicate footer inside the panel, and (b) making the listing's scope and parent/child structure visually legible in both the `/files` box and the Ctrl+T pane — including after a re-root, where every row currently renders flat.

## 2. Why this milestone is ready

- Hard dependencies: none outstanding. Navigation state, the bounded walk, and the daemon listing all closed under `plans/subsystems/tui-editor-correctness-usability-roadmap.md` (M002, C007, C008).
- Interface dependencies are stable and in-tree: `Component`, `SidePanel`, `HostPanel`, `render_hosted`, `FileTreeState`, `dialogs::files::HINT`.
- No unresolved architectural decision. No ADR required — no identity, ownership, protocol, authorization, or migration change.

## 3. Current implementation evidence

Baseline SHA `b77c8156`. All statements below were read at that SHA.

### 3.1 Two copies of the keymap, one of them wrong

`src/tui/components/dialogs/files.rs`:

```rust
pub const HINT: &str =
    "j/k move · ^d/^u page · l/→ root at dir · h/← up · enter open · r refresh · esc close";
```

Its own doc comment: *"Kept in one place so the box, its hosted panel, and the help dialog cannot disagree about what the keys do."* Nothing enforces that. `src/tui/panels/hosted.rs::HostPanel::composer_hints` returns a second, independent string:

```rust
format!("{} · j/k scroll · i act · Shift+← back", self.id.title())
```

`FilesDialog::handle_key` implements no `i` verb, so that line advertises a key that does nothing. `Shift+←` is placement-dependent (`docs/tui.md` §"Moving between the box and the panel": on the right column `Shift+→`/`Shift+L` enters and `Shift+←`/`Shift+H` returns), so the advertised gesture is wrong for a left-column or bottom-row panel.

The footer is painted by `FilesDialog::render` into a reserved interior row, and `render_hosted` renders that same component into the panel column — so a docked panel shows the footer too, in a column that can be as narrow as the hosted pane minimum.

### 3.2 The keymap is hidden whenever the panel is unfocused

`src/tui/panels/mod.rs`:

```rust
pub fn composer_hints(&self, chat_project: Option<&str>, input_mode: InputMode) -> Option<String> {
    if !self.is_focused() { return None; }
    let hints = PanelHints { focused: true, chat_project, input_mode };
    ...
}
```

`is_focused()` is `self.focused && !self.panel_is_empty()`. A populated docked panel that the user has not stepped into advertises nothing. `PanelHints.focused` is hardcoded `true`, so no panel can distinguish the states even if the gate were removed.

### 3.3 The hint row is conditionally reserved

`src/tui/app/render.rs`:

```rust
let hint_rows = u16::from(self.composer_hint_text().is_some());
```

Reserved inside the prompt allocation (the comment above it states carving afterwards "would shrink the input every time a panel took focus"), so an always-present hint costs one transcript row, not input height. `App::composer_hint_text` combines resize mode → chat seed → LLM turn keys → panel hints, returning `None` only when all are absent.

### 3.4 A re-rooted listing renders completely flat

`src/directory_listing.rs::walk_directory` calls `walk(target, relative_base, 0, …)`; `src/tui/file_tree.rs::from_shared_listing` preserves `node.depth` verbatim; `FileTreeState::set_root_prefix` calls `reset_scope()`. Therefore **every row of a scoped listing has `depth == 0`**, and both renderers draw `"  ".repeat(row.depth.min(MAX_INDENT_DEPTH))` — nothing. `MAX_INDENT_DEPTH` is declared independently in both files (`8`).

Within one scope, expansion yields depths 1, 2, …, distinguished only by two spaces per level with no guide glyph.

### 3.5 The host's panel title is overwritten

`render_hosted` paints `Block::borders(ALL).title(" ● Files ")` into `frame.area()`, then calls `widget.render(frame, frame.area(), theme)` on the same rect. `FilesDialog::render` paints its own all-borders block with ` Files (N shown) · <scope> ` into that rect. Same root cause as §3.1: one component, two chrome contracts, no way to tell which it is rendering into. Verify with a rendered-buffer test before changing it.

## 4. Invariants that must not regress

- The picker performs **no** file read and holds no document text. Opening a row remains `TuiMsg::FilesOpenSelected` → `open_editor`.
- `Enter` on a directory expands in place; `l`/`→` re-roots the listing. They stay distinct verbs.
- `FileTreeState::root_history` stays the only scope axis; `ascend_root`/`set_root_prefix`/`reset_scope` semantics unchanged.
- Generation, request-id, and prefix-echo fences in `FileTreeState::apply_listing` unchanged; a stale or foreign completion is still dropped.
- The bounded walk (`MAX_LISTING_DEPTH`, `MAX_LISTING_NODES`, `MAX_LISTING_EXAMINED`, `MAX_LISTING_NAME_BYTES`) unchanged; `local_walk_root` still refuses a non-local/symlink root.
- The composer input height and the transcript `Min(10)` floor do not change.
- `CODEGG_FILE_ICONS=plain` stays a complete, readable fallback.
- No storage, protocol, DTO, daemon, or authorization change. `STORAGE_LAYOUT_VERSION` unchanged.

## 5. Scope

**In:**

1. A widget-owned keymap accessor on `Component`, defaulted so no existing implementor changes behavior.
2. `HostPanel::composer_hints` sourcing the hosted widget's keymap, falling back to the current generic line.
3. Composer hint visibility for a **populated** panel regardless of focus, with real focus state threaded into `PanelHints` and a placement-correct "take the keyboard" gesture.
4. A panel-render entry point on `Component`, used by `render_hosted`, so a hosted box does not paint modal chrome.
5. One shared display-depth computation + tree-guide formatter used by both `/files` and Ctrl+T.
6. A breadcrumb for the current scope in the box title area.
7. Docs and tests.

**Out:** anything in roadmap §3 (non-goals) — no new keys, no `..` row, no typeahead, no mutations, no protocol work, no M002 item.

## 6. Required production changes

### 6.1 `src/tui/components/component.rs` — two defaulted trait methods

Add to `trait Component`, both with defaults so the ~50 implementors are untouched:

```rust
/// The verbs this widget owns, for the host to advertise on the composer
/// hint line. `None` means "the host's generic line is correct for me".
fn keymap_hint(&self) -> Option<String> { None }

/// Render into a side-panel column. Defaults to the modal render; a widget
/// that draws its own footer, border, or title overrides this instead of
/// `render`, so modal chrome never leaks into the column.
fn render_in_panel(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>) {
    self.render(frame, area, theme);
}
```

Document both next to `docks_as_viewer`, which is the existing precedent for "the host needs to know this widget's shape".

### 6.2 `src/tui/components/dialogs/files.rs`

- `keymap_hint()` returns `Some(HINT.to_string())`. This is the moment the constant's doc comment becomes true.
- `render_in_panel()` renders listing-only: no footer row, no `Block::borders(ALL)` of its own, no title. The host already painted border and title, and its focus dot must survive.
- Keep `render()` (modal) unchanged in shape — footer and title stay for the box.
- Both paths must share one row-formatting call so the two presentations cannot drift.

### 6.3 `src/tui/panels/hosted.rs`

- `render_hosted` calls `widget.render_in_panel(...)` instead of `widget.render(...)`.
- `composer_hints` becomes: if the widget answers `keymap_hint()`, use it; otherwise keep the existing generic line verbatim. Add the panel title as the prefix so the line still names the surface.

### 6.4 `src/tui/panels/mod.rs` — focus-independent hints

- `SidePanelHost::composer_hints`: drop the `is_focused()` early return. Gate on "the active panel has content" (`!panel_is_empty()`) instead, so an **empty** panel still advertises nothing — this is what keeps `tests/tui_chat_surfaces.rs` cases `no_panel_focused_advertises_no_keys` and `an_unbound_panel_advertises_nothing_and_takes_no_focus` passing.
- Pass the real `self.is_focused()` into `PanelHints.focused` instead of the hardcoded `true`.
- Add `placement: PanelPlacement` to `PanelHints` so an unfocused panel can name the correct entry gesture (`Shift+→`/`Shift+L` right column, `Shift+←`/`Shift+H` left column, `Shift+↓`/`Shift+J` bottom row — `docs/tui.md`).
- Update the `composer_hints` doc comment: it currently promises the line renders "while it holds the keyboard", which is no longer the rule.
- The other panels (`session`, `chat`, `terminal`, `help`) keep their existing hints; their `composer_hints` must keep working when `focused` is now genuinely `false`. Any panel whose hint text asserts "these keys work" must gain a focus-qualified form rather than continuing to advertise verbs the keyboard will not deliver.

### 6.5 Listing hierarchy — one formatter, two callers

Add to `src/tui/app/state/file_tree.rs` (state owns `root_prefix()`; renderers own no arithmetic):

```rust
/// Depth to *display* for `row`: components already consumed by the current
/// scope, plus this row's depth within it. A re-rooted walk restarts depth at
/// 0, so without this every row of a scoped listing renders flush-left.
pub fn display_depth(&self, row: &TreeRow) -> usize
```

Backed by a scope-depth derived from `root_prefix()` (count `/`-separated components), clamped by a single `MAX_INDENT_DEPTH` now defined once — delete the duplicate `const MAX_INDENT_DEPTH: usize = 8;` from both `src/tui/components/dialogs/files.rs` and `src/tui/components/file_tree.rs`.

Add one formatter, e.g. `pub fn indent_prefix(depth: usize, last: bool) -> String` in the same module or a sibling of `src/tui/file_icons.rs` (which is already documented as "one table, two callers"):

- depth 0 → `""`
- depth ≥ 1 → box-drawing guide: `│  ` continuation and `├─` / `└─` for the row itself, **BMP box-drawing only** (no Nerd Font code points — a plain-icon terminal must stay readable).
- Cap total guide width; elide the guide before the name, never the reverse.

Use it in both `src/tui/components/dialogs/files.rs::render_rows` and `src/tui/components/file_tree.rs` so `/files` and Ctrl+T agree.

### 6.6 Breadcrumb

In the `/files` box, render the scope as a breadcrumb (`workspace › src › tui`) alongside the existing title, derived from `root_prefix()`. Keep the existing total-count and workspace-name facts; this replaces the single `scope` string in the title, so update `the_title_names_the_directory_the_listing_is_rooted_at` accordingly rather than layering a second scope string on top.

## 7. Ordered work packages

**W1 — Establish the failing evidence first.** Before any production change, add tests that fail at baseline and are the acceptance target: (a) a rendered-buffer test proving a hosted files panel paints `HINT` inside the column; (b) a test proving `composer_hint_text()` is `None` for a populated-but-unfocused docked files panel; (c) a rendered test proving a listing scoped with `l` renders every row at column 0 (drive it through `set_root_prefix("dir")` + a rebuilt listing); (d) a test proving the host's ` ● Files ` title is overwritten. Each must assert buffer content, not reachability.

**W2 — `Component` seam** (§6.1). Defaults only; `cargo check` must show zero implementor changes.

**W3 — Keymap authority** (§6.2 first half, §6.3). `FilesDialog::keymap_hint` + host sourcing + fallback. W1(a)/(b) partially addressed; the fallback must keep every other panel's line byte-identical.

**W4 — Always-on hints** (§6.4). Remove the focus gate, thread real focus + placement, update every panel hint that assumed `focused == true`. Re-run the existing `tests/tui_chat_surfaces.rs` hint cases — the two empty/unbound ones must still pass unchanged.

**W5 — Hosted chrome** (§6.2 second half). `render_in_panel` + `render_hosted` switch. W1(a)/(d) must now pass; the modal box footer must be untouched.

**W6 — Hierarchy** (§6.5, §6.6). `display_depth`, one `MAX_INDENT_DEPTH`, one guide formatter, both renderers, breadcrumb. W1(c) must now pass. Verify the 22-column pane case explicitly.

**W7 — Docs** (§12). `docs/tui.md` and `architecture/tui.md`.

**W8 — Full sweep** (§10, §11). Panel-id sweep, `verify.sh quick`, `verify.sh full`.

## 8. Failure, cancellation, restart, contention semantics

None of this work issues a request, so none of these change. The listing's existing semantics are load-bearing and untouchable: a completion for a stale generation, a spent request id, or a mismatched `prefix` echo is dropped; a superseded blocking walk exits at its next entry via the cooperative flag; at most one scan runs per side. M001 must not add a debounce, a poll, or a local filter that would change what a refresh shows.

The one behavior that legitimately changes is visible: a re-rooted listing now indents by scope depth. That is display arithmetic over an unchanged listing.

## 9. Compatibility and migration

- No migration. No persisted format touched. `SidePanelId::config_key` spellings unchanged, so existing persisted `Ctrl+digit` panel bindings keep resolving.
- No protocol or DTO change; the daemon's `prefix` echo remains the only scope authority.
- User-visible behavior change, by design: hint line appears for a docked populated panel that is not focused; a tree guide replaces bare spaces; a breadcrumb replaces the flat scope string in the title. All three are documented in §12.

## 10. Required tests

Focused (new, in `src/tui/components/dialogs/files.rs` and `src/tui/panels/`):

1. `a_hosted_files_panel_omits_its_modal_footer` — rendered buffer, the column's last interior row is not `HINT`.
2. `the_modal_box_still_shows_its_footer` — the counterpart, so W5 cannot regress the box.
3. `the_hosted_panel_title_survives_the_widget_paint` — the ` ● Files ` title cell is present after hosting.
4. `the_panel_keymap_comes_from_the_widget_not_the_host` — `keymap_hint()` content reaches the hint line; the generic `i act` line is gone for Files.
5. `a_populated_unfocused_panel_still_advertises_its_keys` — `composer_hint_text()` is `Some` with `is_focused() == false`.
6. `an_empty_panel_advertises_nothing` — placeholder/unbound panel unchanged.
7. `the_unfocused_hint_names_the_placement_correct_gesture` — left column, right column, bottom row each name the right gesture.
8. `a_scoped_listing_indents_under_its_scope` — `set_root_prefix("a/b")` ⇒ rows render at depth 2.
9. `expanded_children_nest_under_their_parent` — depth 0/1/2 render monotonically indented.
10. `the_guide_is_box_drawing_and_needs_no_nerd_font` — every guide char is in the BMP box-drawing range.
11. `a_narrow_column_elides_the_guide_before_the_name` — 22-column pane keeps the name.
12. `display_depth` table test — workspace root, one level, deep scope, clamp.

Integration (`tests/`):

13. Extend `tests/tui_panel_docking.rs` or `tests/tui_usability_fixes.rs`: dock `/files` with `Shift+L`, assert the composer hint row carries the file verbs and the column carries no footer.
14. `tests/tui_render.rs`: the breadcrumb renders and the title facts still hold.
15. Regression sweep: for every `SidePanelId::ALL`, a populated focused panel's hint line is unchanged from baseline except Files.

Existing tests that must keep passing **unmodified**: `tests/tui_chat_surfaces.rs::no_panel_focused_advertises_no_keys`, `::an_unbound_panel_advertises_nothing_and_takes_no_focus`, and the `dialogs::files` unit tests except the title test that §6.6 intentionally changes.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tui::components::dialogs::files
cargo test -p codegg --lib tui::panels
cargo test --test tui_panel_docking --test tui_usability_fixes --test tui_chat_surfaces --test tui_render
cargo fmt --check
scripts/verify.sh quick
scripts/verify.sh full
```

`verify.sh full` before the closure record, labeled truthfully as local if not run in CI.

## 12. Documentation updates

- `docs/tui.md` §"Browsing files": document that a docked panel advertises its keys on the composer status line (not in the panel), that the line persists while the panel is unfocused and names the gesture that takes the keyboard, the tree-guide rendering, and the breadcrumb. Update the panel gesture table's cross-reference if the hint wording changes.
- `docs/tui.md` §"Browsing files": state plainly that keys are unchanged — M001 adds none.
- `architecture/tui.md` §"Workspace file manager (`/files`)": replace the `Enter`/`l` key table with the unchanged contract plus the new chrome contract (modal footer vs panel-hosted chrome, host title authority), and document `display_depth` and the shared guide formatter. Update `## Verified counts` only if a listed count actually changed.
- `architecture/overview.md` only if a module map entry changes.
- The `tui` skill (`architecture/tui.md` is its authoritative doc) if a panel-hint rule changes.

## 13. Acceptance criteria (externally observable)

1. `/files` then `Shift+L`: the panel column shows no keymap footer; the row directly above the composer shows the file verbs (`j/k`, `^d/^u`, `l/→`, `h/←`, `enter`, `r`).
2. That line is present immediately after docking, before any `Shift+→`, and remains when focus moves away; it names the gesture that takes the keyboard for the current placement and none of its verbs is `i act`.
3. `/files` with no panel docked is unchanged: the box still shows its own footer.
4. Every other hosted panel's hint line is byte-identical to baseline.
5. A placeholder or unbound panel still shows no hint and takes no focus.
6. After `l` into `a/b`, the listing's top-level rows are drawn indented under the scope, and a breadcrumb reads `… › a › b`; the last visible file is not half-hidden (footer removal frees the row).
7. Expanding a directory in place nests its children under it, visibly, in both `/files` and the Ctrl+T pane.
8. The guide renders identically with `CODEGG_FILE_ICONS=plain`.
9. A 22-column pane shows names, eliding guides first.
10. No new daemon request, no protocol/storage/auth change, `STORAGE_LAYOUT_VERSION` unchanged.

## 14. Stop conditions

Stop and report rather than improvise if:

- The hint row's always-on reservation cannot be made without changing composer input height or the transcript `Min(10)` floor. Report the measured geometry; do not trade input height for the line.
- Removing the modal footer appears to require changing `Enter`/`l` semantics, or `leave_selected`/`ascend_root` behavior.
- Any fix appears to require the picker to read the filesystem locally, or to filter an already-loaded listing instead of re-walking (that would silently present a bounded partial subtree as the directory).
- A tree guide cannot be made to fit the 22-column pane without hiding names — propose a non-guide alternative and stop.
- The two renderers cannot be made to share one depth/guide function (e.g. a borrow or module-cycle obstacle) — report it rather than shipping two copies.
- Any temptation to add a key, a `..` row, or typeahead: that is M002, not M001.

## 15. Closure evidence required

- `cargo fmt --check`, `verify.sh quick`, and `verify.sh full` results, labeled local vs CI.
- The 15 tests in §10, with the three focused-baseline failures from W1 shown failing before the change and passing after.
- Rendered-buffer output (or an assertion naming the exact cells) for: hosted panel without footer, modal box with footer, host title surviving, and a scoped listing's indentation.
- Confirmation that the two existing empty/unbound hint tests pass unmodified.
- Confirmation that no file under `src/directory_listing.rs`, `crates/codegg-protocol/`, or any migration file changed (`git diff --stat` evidence).
- `docs/tui.md` and `architecture/tui.md` updated and consistent with the shipped behavior.
- An unblock audit against the registry's blocked section.

## 16. Handoff notes

- Start at W1. The three failing tests are the milestone's real acceptance signal; a change that does not turn them green has not solved the reported problem.
- `Component` has two new defaulted methods. Do not make either required — that would touch every implementor for no reason.
- The single most likely regression is another duplicated string: resist writing a Files-specific branch anywhere outside `src/tui/components/dialogs/files.rs`.
- `display_depth` is display arithmetic over an unchanged listing. If it starts changing `state.expanded`, `state.selected`, or what a walk returns, it has crossed into navigation — stop.
- Baseline note: the worktree also carries an unrelated local modification to `src/tui/editor.rs`. Do not fold it into this milestone.
