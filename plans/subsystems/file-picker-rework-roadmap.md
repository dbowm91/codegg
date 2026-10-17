# File Picker Rework Roadmap

Status: conditionally closed (M001 implemented, verified, and committed on `main`, closure record written; M002 candidate, not approved for planning)

Baseline: `main` at `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`, 2026-10-16. Re-inspect HEAD before each implementation.

Parent: `plans/subsystems/tui-editor-correctness-usability-roadmap.md` (closed 2026-10-13; M006-D file navigator, M002/C007 daemon-scoped listing).

Long-term references:
- `plans/000-long-term-specification.md` — TUI-first product; correctness before transparent magic; locality of authority.
- `plans/001-terminology-and-domain-model.md` — document attachment, workspace-relative path identity.
- `plans/002-long-term-roadmap.md` — frontend-neutral projections, TUI evolution.
- `plans/003-planning-process.md` — milestone sizing, handoff review, registry discipline.

Related ADRs: none required. This workstream changes no identity, ownership boundary, protocol, or authorization semantics. ADR-0022 (daemon-authorized workspace directory listing) and ADR-0033 (directory-tied project identity) are consumed, unchanged.

## 1. Purpose and ownership boundary

Make the workspace file navigator usable and intuitive on the scale of other TUI file pickers, without touching what it *is*.

The picker is one implementation with three presentations: the `/files` modal box (`src/tui/components/dialogs/files.rs`), the same box hosted in a side panel (`src/tui/panels/hosted.rs`), and the editor-side Ctrl+T pane (`src/tui/components/file_tree.rs`). All three share one `FileTreeState` (`src/tui/app/state/file_tree.rs`) and one bounded walk (`src/tui/file_tree.rs` → `src/directory_listing.rs`). The rework owns **presentation and discoverability only**:

- The navigator stays a navigator. It never reads file bytes; opening a row remains `TuiMsg::FilesOpenSelected` routed to `open_editor`.
- The walk stays daemon-authorized, bounded, generation-checked, and prefix-echo-checked. M001 adds no walk, no protocol field, no storage, no migration.
- `Enter` (expand in place) and `l`/`→` (re-root the listing) stay **different verbs**. This is a deliberate, documented contract, not an oversight to be tidied away.
- `FileTreeState::root_history` stays the single scope axis. M001 adds no second scope representation.

Ownership boundary: `src/tui/components/dialogs/files.rs`, `src/tui/components/file_tree.rs`, `src/tui/panels/{mod.rs,hosted.rs}`, `src/tui/components/component.rs`, and the composer hint line in `src/tui/app/render.rs`. Not in boundary: the walk, the daemon, storage layout, authorization.

## 2. Work classification

**Invariants.** No local filesystem read in the picker. No second copy of the panel keymap that can drift from the widget that owns the keys. No in-panel keymap footer on a docked panel. No `depth`-derived rendering that disagrees with `root_prefix()`. No layout change that shrinks the composer input to make room for a strip.

**Capabilities.** A docked file panel that advertises its real keys on the shared status line above the composer, always, whether or not it holds the keyboard. A listing where the current scope and every row's position under its parent directory are legible without reading names. A picker whose hierarchy survives a re-root (`l`) instead of flattening to a single flush-left column.

**Infrastructure.** A widget-owned keymap accessor on the `Component` trait; a panel-hosted render entry point distinct from the modal render path; one shared display-depth/tree-guide formatter consumed by both renderers.

**Polish.** Breadcrumb line for the current scope, tree-guide glyphs, hint wording.

## 3. Non-goals

- No change to the key contract of `j/k`, `Ctrl+d/u`, `Enter`, `l/→`, `h/←`, `r`, `Esc`. No new keys in M001. No rebinding `Enter` to descend-and-open, no `..` parent row, no typeahead filter, no fuzzy find, no multi-select, no mkdir/delete/rename, no drag-and-drop.
- No new remote-filesystem browsing protocol. The `local_walk_root` refusal in `src/tui/file_tree.rs` stays a refusal.
- No change to `FileTreeState` public semantics, the generation/prefix fences, or `apply_listing` drop rules.
- No change to `SidePanelId` variants, `config_key` spellings, or persisted panel bindings.
- No Nerd Font requirement for the hierarchy. Tree guides are box-drawing (BMP, universal); the `CODEGG_FILE_ICONS=plain` fallback must remain fully readable.
- No change to `/open`, `@`-mention indexing, or the Ctrl+T pane's `MIN_TREE_PANE_WIDTH` split policy.

## 4. Current state (repo evidence at baseline)

### 4.1 A docked panel paints its keymap twice, and the wrong one wins

`FilesDialog::render` (`src/tui/components/dialogs/files.rs`) reserves one interior row and paints `dialogs::files::HINT` at the bottom of the box. That footer is correct for the modal. `render_hosted` (`src/tui/panels/hosted.rs`) renders the *same* component into the panel column, so the docked panel shows that footer too — inside a column that is often 22–40 columns wide, where it wraps or truncates.

`HostPanel::composer_hints` (`src/tui/panels/hosted.rs`) advertises a hardcoded line:

```text
{title} · j/k scroll · i act · Shift+← back
```

For the files panel this is wrong in three ways: `i act` is not a verb `FilesDialog::handle_key` implements, `Shift+← back` names a gesture whose meaning is placement-dependent (`docs/tui.md`, "Moving between the box and the panel"), and the line is a second copy of the keymap that can drift from `dialogs::files::HINT` — the one constant whose own doc comment claims it exists "so the box, its hosted panel, and the help dialog cannot disagree about what the keys do."

### 4.2 The panel keymap is hidden whenever the panel does not hold the keyboard

`SidePanelHost::composer_hints` returns `None` unless `is_focused()`, and `is_focused()` is `self.focused && !self.panel_is_empty()`. So a docked, populated files panel that the user has not yet stepped into advertises nothing. The keys are discoverable only by opening the box first, which defeats docking.

`PanelHints.focused` is hardcoded to `true` at the dispatch site (`src/tui/panels/mod.rs`), so no panel can currently distinguish the two states anyway.

### 4.3 The hint row is not reserved when it is absent

`let hint_rows = u16::from(self.composer_hint_text().is_some());` (`src/tui/app/render.rs`) reserves the row inside the prompt allocation. Making the hint always present while a panel is docked therefore costs one **transcript** row, not input height — which is the existing documented intent of that carve. M001 must prove this rather than assume it.

### 4.4 A re-rooted listing is completely flat

`walk_directory(target, relative_base, cancel)` calls `walk(target, relative_base, 0, …)` (`src/directory_listing.rs`), so a subtree walk restarts depth at 0 and `from_shared_listing` preserves `node.depth` verbatim. `FileTreeState::set_root_prefix` then calls `reset_scope()`, replacing the listing. Consequence: **every row of a scoped listing renders at `row.depth == 0`**, and `"  ".repeat(row.depth)` (`src/tui/components/dialogs/files.rs`, `src/tui/components/file_tree.rs`) draws nothing. After `l` the column is uniformly flush-left; the only hierarchy signal left is the scope text in the title.

Within one scope, in-place expansion does produce depths 1, 2, …, but the only cue is two spaces per level with no guide glyph, so "these rows are under this directory" is carried entirely by the folder icon.

### 4.5 The hosted panel's own title is painted over

`render_hosted` paints `Block::borders(ALL).title(" ● Files ")` into `frame.area()` and then calls `widget.render(frame, frame.area(), theme)` on the **same** rect. `FilesDialog::render` paints its own all-borders block with its own title into that rect, so the second paint overwrites the host's border and title cells. The panel's visible chrome is the dialog's, and the host's focus dot (`●`) is never seen. This is the same root cause as §4.1: one component with two chrome contracts and no way to tell which one it is rendering into.

## 5. Target architecture

One seam, two questions:

1. **What keys does this widget own?** `Component` gains a keymap accessor with a `None` default, so the other hosted boxes keep their existing generic line and nothing regresses. `FilesDialog` answers from `dialogs::files::HINT` — the constant that is already documented as the single source. `HostPanel::composer_hints` reads the widget first and falls back to the generic line. The keymap is now written once, next to the `handle_key` arms that implement it.
2. **What chrome does this widget draw?** `Component` gains a panel-render entry point that defaults to the modal render. `render_hosted` calls the panel entry point. `FilesDialog` overrides it: no footer row, no second border, and the host's title/focus dot survives.

The composer hint line becomes the one status surface for panel keys: it shows the docked panel's own keymap whenever that panel holds content, carries the real focus state, and names the placement-correct gesture for taking the keyboard when it does not.

The listing gains one shared display-depth computation and one tree-guide formatter, used by both the `/files` box and the Ctrl+T pane, so `l` visibly indents the new scope and expanded children visibly nest under their parent.

## 6. Dependency graph

| Dependency | Class | State |
|---|---|---|
| `FileTreeState` navigation, `root_history` scope axis | interface | closed — `tui-editor-correctness-usability` M002/C007 |
| Daemon-authorized bounded listing, prefix echo, generation fence | interface | closed — ADR-0022, C007/C008 |
| `SidePanel` / `HostPanel` / `render_hosted` host seam | interface | closed — `desktop-frontend-ide-foundation` M006 |
| `Component` trait and modal stack | interface | closed — stable, 50+ implementors |
| Composer hint line and its `hint_rows` carve | interface | closed — landed with panel hints |

No hard dependency is outstanding. No ADR is required. M001 is dependency-ready.

## 7. Milestones

### M001 — Panel keymap authority and listing hierarchy (conditionally closed — see `plans/closure/file-picker-rework/001-status.md`)

**Class:** capability (with polish).

**Objective:** One written copy of the picker's keymap, shown on the composer status line whenever the panel is docked and populated; no in-panel footer; a listing whose scope and parent/child structure are legible in both presentations.

**Deliverable boundary:** `Component` seam additions, `HostPanel` key sourcing, focus-independent composer hints with placement-correct gestures, panel-render entry point, shared display-depth + tree-guide formatter, breadcrumb, `/files` + Ctrl+T parity, docs.

**Exit conditions:** see `plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md` §13.

### M002 — Picker ergonomics beyond M001 (candidate; NOT approved for planning)

Deliberately not opened. Candidates observed during M001 research, each requiring its own planning pass before it may be scheduled:

- A typeahead/filter field scoped to the current root, with the filter text bounded and cleared on re-root.
- A `..` parent row, or an explicit "up" affordance for users who do not know `h`.
- Whether `Enter` on a directory should also be reachable by a key that opens it in the editor (`l` then `Enter` is the current two-step).
- Narrow-column behavior past the point where a tree guide costs more width than it returns.

No M002 work may begin while M001 is open, and none of it may be smuggled into M001.

## 8. Cross-cutting requirements

- **Storage/protocol:** none. `STORAGE_LAYOUT_VERSION` unchanged. No DTO field added. The daemon echo of `prefix` remains the only scope authority.
- **Security/authorization:** none changed. The picker performs no read; `local_walk_root` refusal unchanged; no `file.read` relaxation.
- **Concurrency:** unchanged. M001 adds no request. The generation/prefix fences and cooperative cancel flag are untouched.
- **Layout:** one transcript row may be reserved whenever a populated panel is docked. The composer input height and the `Min(10)` transcript floor must not change.
- **Docs:** `docs/tui.md` ("Browsing files", panel gesture table) and `architecture/tui.md` (`/files` module map, verified counts if any) must land with the code.

## 9. Verification strategy

- Focused: `cargo test -p codegg files` style filters for the box, plus new rendered-buffer assertions for both presentations.
- `scripts/verify.sh quick` is the gate; `verify.sh full` before closure.
- Render tests must assert **buffer contents**, not just that a string is reachable: a docked panel's footer row must be absent from the column's last interior row, and the hint line must carry the panel's own verb list.
- A sweep over `SidePanelId::ALL` proving the keymap accessor default keeps every non-files panel's hint line unchanged.

## 10. Risks and decision points

- **Trait surface growth.** `Component` already has many optional methods; two more is consistent but must stay defaulted so no implementor is forced to change.
- **Always-on hint line changes a rendered invariant.** Anything asserting "no hint row without focus" must be reviewed; `tests/tui_chat_surfaces.rs` cases for an *empty/unbound* panel must keep passing (they should: no content, no hint).
- **Hint line width.** Real keymaps are longer than `j/k scroll · i act`. The line already ellipsizes to `area.width - 4`; M001 must keep the most load-bearing verbs at the front so truncation degrades gracefully.
- **Guide width in a 22-column pane.** `MIN_TREE_PANE_WIDTH` is 22. A tree guide must be capped and elided before a name is, never the reverse.
- **Two renderers drifting again.** The depth/guide formatter must be one function with one test table, not a helper each renderer reimplements.

## 11. Completion definition

M001 closes when a docked `/files` panel shows no in-panel keymap, the composer status line carries the picker's real verbs whenever the panel is populated (focused or not) with a placement-correct way to take the keyboard, a re-rooted listing is visibly indented under its scope and expanded children visibly nest under their parent in both the box and the Ctrl+T pane, the other hosted panels are unchanged, `verify.sh quick` and `verify.sh full` are green, and `docs/tui.md` plus `architecture/tui.md` match the shipped behavior. The roadmap then closes with the roadmap's own completion definition met; M002 is never implicitly approved.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready for handoff | `plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md` | — | none |
| M002 | not started (not approved) | — | — | requires fresh planning |