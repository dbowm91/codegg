# Live evidence — M002, M005, M008, M012

Date: 2026-11-26. Branch `feat/auto-20261126-cae46b00`, head `9f8274622`.
Parent plan: `plans/subsystems/tui-workspace-and-panel-surface-corrective-addendum.md`.

## What ran

- `cargo test -p codegg-tui --lib` — 3326 passed, 0 failed, 1 ignored.
- `scripts/verify.sh quick` — passed at `e48908cc1` (rustfmt, generated assets,
  guards, workspace check). After the later commits the individual guards
  (command catalog, frontend action set, TUI transport authority, test assertion
  quality, test duplication, comment budget) and the full `codegg-tui` lib suite
  pass, and `scripts/verify.sh quick` was re-run on this head.
- A live `tui-test` session against the product binary.

## Live session

- Binary: `target/debug/codegg`, built from this head (`cargo build -p codegg-cli --bin codegg`).
- Terminal: 130 columns x 40 rows, alacritty backend.
- cwd: `/tmp/codegg-receipt`, a fresh `git init` repository with one commit.
- Commands: `tui-test open --cols 130 --rows 40`, `tui-test submit '<binary>'`, `tui-test wait idle`, `tui-test expect text 'has nothing to show yet'`, `tui-test screenshot`.

## Asserted

- The TUI launches in a fresh repository and paints the panel column.
- The idle panel body reads `Logs has nothing to show yet.` with the `daemon · notices` hint — the M002 one-line state.
- No "Nothing loaded" and no "Open it from the panel manager (⇧S)" appears anywhere on the frame.

Receipt: `receipt-panel-idle.svg` (this directory).

## Not captured, and why

The remaining receipts in the parent plan §9 — `/sessions` scope, `/workspace` navigation,
the lifecycle keys, the launch sheet — were not captured in this session. After
launch the panel column owns the keyboard, so the scripted `tui-test submit` keystrokes
landed in the panel instead of the composer and the frames show the panel rather
than the surface under test. The two screenshots taken after that point
(`/tmp/receipt-b-sessions.svg`, `/tmp/receipt-launch.svg`) are therefore **not** evidence and are not
retained here. A follow-up session needs the explicit step that returns focus to
the composer before typing a `/` command.

## Unit coverage in place of those receipts

- M002: every panel in `SidePanelId::ALL` keeps the column; a matching box is docked
  rather than mounted; an unrelated box is left mounted.
- M005/M012: lineage grouping across two workspaces, the empty-row filter with the
  active-session exemption, and the cleanup count refusing unanswered counts.
- M008: edit pins the selection, archive arms then performs, another key disarms,
  destroy opens the counted confirmation, the no-daemon team path names its denial.
