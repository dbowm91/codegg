# M006 — Route input owns the capitals: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- `Shift+X` / `Shift+H` / `Shift+L` are tree claims, not route-wide claims: moved **below** every text-owning branch in `handle_workspace_view_key` (inline project picker, add-member form, launch sheet, Panel composer, docked-composer draft) and gated on `focus == WorkspaceInputFocus::Tree && mode == WorkspaceMode::Normal`. A shifted letter now reaches whatever input the user is typing into, and the three capitals still beat lowercase `x` in the tree.
- Regression tests press keys through `App::on_key`, not the router directly, so the branch order is what they measure: `the_route_composer_owns_shifted_letters` (a shifted letter typed into the route composer lands in the buffer and the route stays open) and `shift_x_leaves_only_from_the_normal_tree` (leave fires only from the normal tree).
- Both tests were proven red with the production change reverted and the tree restored byte-identical (patch applied back, `shasum -a 256` of `git diff -- <files>` unchanged).
- Docs corrected in the same change: the route-mode paragraph in `architecture/tui.md`, the route intro in `docs/tui.md`, and the stale rationale comment in `tui_composer_claims.inc`.

## Commands run

- `cargo test -p codegg-tui --lib workspace_dashboard` — 41 passed at this milestone.
- `cargo test -p codegg-tui --test tui_render` (unfiltered) — 1247 passed / 9 failed (the eight recorded pre-existing failures plus M010's then-red test).
- `cargo fmt --all -- --check`, `git diff --check` clean.
- Live receipt: `Shift+H` moves the header to `NOR · composer` with the `[INS][CHAT]` badge, `Shift+X` typed there yields `HelloX` with the route still open, and `Shift+X` from the tree leaves the route — `plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md`, frames `receipts/m006-route-surface.png` and `receipts/m006-composer-focus.png`.
