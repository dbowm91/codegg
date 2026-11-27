# M002 / M003 live TUI receipt

Date: 2026-11-26. Used the already-built target/debug/codegg binary; no build was run. No product code was changed. The disposable fixture was created at /tmp/codegg-receipt-m2m3/repo, with HOME=/tmp/codegg-receipt-m2m3/home and CODEGG_DATA_HOME=/tmp/codegg-receipt-m2m3/data.

## Commands and setup

The fixture setup followed the requested sequence: remove /tmp/codegg-receipt-m2m3; create repo and home directories; git init -q -b main; write one.txt=alpha and two.txt=beta; commit with r@example.com / r; then modify the files to alpha changed and beta changed. Launch command:

tui-test run --session m2m3 --cwd /tmp/codegg-receipt-m2m3/repo --cols 130 --rows 40 --env HOME=/tmp/codegg-receipt-m2m3/home --env CODEGG_DATA_HOME=/tmp/codegg-receipt-m2m3/data -- /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261126-cae46b00/target/debug/codegg

Panel loads used submit /panels, key press i, type <panel name>, key press Enter, wait idle, then text. Diff was tested with submit /diff and submit '/diff one.txt'. The required tui-test close --session m2m3 completed successfully.

## Results

### A — M002 fresh-launch idle body: FAIL

Fresh screen showed “Type a prompt to begin” and a right-side “Session” card with “no session” and “Git / not a git repo”. It did not show “<label> has nothing to show yet.” or the requested inline idle body and hint. It did not show either forbidden phrase (“Nothing loaded” / “Open it from the panel manager”). Fixture status was independently observed as ## main, M one.txt, M two.txt.

Screenshot: plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m002-launch-idle.svg.

### B — Terminal NOR → INS and typing: FAIL

Focused Terminal showed [NOR] and the exact hint “⇧Q leave · TERM · i start a shell · v chars · x lines · d delete · space+y copy · C clear”. Initially the body showed “No active session; open a project session before creating a terminal”. Pressing i left the badge at [NOR]; typing “echo RECEIPT” and Enter did not produce RECEIPT and another copy of the same error appeared. A delayed screen did later show a shell banner and bash-3.2$ in the terminal area, but the focused panel still showed [NOR]. A subsequent explicit i test again left NOR and showed the no-active-session error. No successful typed command was observed.

Screenshots: receipts/m002-terminal-nor.svg (NOR and error); receipts/m002-terminal-ins.svg (captured after failed i transition; it is still a NOR/error frame, not an INS success frame).

### C — Panel walk: FAIL (partial)

- Session: first filtering selected the Tree result, which showed “no forks”. A later attempt selected the separate Sessions entry. The exact Session panel body was not conclusively demonstrated.
- Chat: real panel content showed “#general 0/0” and “no messages — press i to write”.
- Terminal: error box “No active session; open a project session before creating a terminal”; later a delayed shell banner/prompt was visible, but input mode still failed.
- Workspace: project row “repo” was selected; hint included “⇧Q leave · WORKSPACE · j/k move · Enter open · i filter · Shift+← back”.
- Help: real keybinding content showed “Normal Mode” and rows including “i Insert”.
- Sessions: switcher showed “(no sessions loaded)” for project repo. Its “w new session in this checkout” action showed “This session runs in the project's own root — it has no managed checkout to reuse”.
- Tasks: inline body “Tasks has nothing to show yet.” with hint “running · waiting”.
- Logs: real daemon notices included “command's workspace could not be resolved, so its run cannot be recorded” and repeated “No active session; open a project session before creating a terminal” errors.

Representative screenshot (Tasks idle body): plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m002-panel-walk.svg.

### D — M003 Diff aggregate, file patch, single file: PARTIAL / FAIL

- Aggregate /diff: PARTIAL. It opened Diff Viewer with title “Diff: Repository Diff | Inline”. Visible patch text contained both “diff --git a/one.txt b/one.txt” and “diff --git a/two.txt b/two.txt”, and lines “-alpha / +alpha changed” and “-beta / +beta changed”. The combined inline patch showed no file list or explicit additions/deletions counts, so the requested list/count confirmation was not achieved.
- First-file selection / separate patch: FAIL. Overlay controls were “Esc close · j/k/↑/↓ scroll · PgUp/PgDn page · s side-by-side”, not file selection controls. Down then Enter closed the overlay; reopening showed aggregate patch, not a selected-file detail view.
- Single-file /diff one.txt: PASS. Title “Diff: one.txt | Inline” showed “alpha” and “alpha changed” only.

Screenshots: receipts/m003-diff-aggregate.svg (aggregate); receipts/m003-diff-file.svg (aggregate patch re-opened, not per-file selection); receipts/m003-diff-single.svg.

## Blockers / remaining risks

The isolated app data home restored a prior tab on a same-session relaunch and warned “restored tabs did not include the directory you launched in (.../repo); kept the launch workspace instead”; the side panel then reported “not a git repo” despite the fixture Git status. Sessions showed no loaded sessions and no managed checkout to reuse, blocking Terminal input. A prompt attempt also surfaced “provider_not_found: Provider '' not found” and did not establish a session. No product files were modified.
