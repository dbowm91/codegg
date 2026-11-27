# M004 — Lifecycle and launch as main-panel modes: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- Register/Edit/State/Destroy/Team/launch render full-size in the main body: view-owned inline picker (`workspace_inline` flag, `handle_workspace_picker_key`) reusing daemon request functions/state machines; launch placement/base/branch rendered in-panel (not footer-only); route never calls `begin_lifecycle_from` into a modal on this path.
- Destroy names session count; archive arms-then-performs; token plaintext never enters panel state (`DeviceSecretDialog` untouched).
- TUI never mints paths/branches; one idempotency key per submit (retry reuses, refusal rotates).
- Global `Shift+F` / `Ctrl+backslash` picker hotkeys keep working outside the route (explicit fall-through arms in the route router; convergence tests green).
- Catalog ownership guard green (TUI descriptions restored to canonical daemon wording; `/worktree` selector detail lives in TUI docs, not catalog copy).

## Commands run

Same gate set as 001-status; `check_command_catalog_ownership.py` clean; `docs/tui.md` + `architecture/tui.md` updated in the same change.
