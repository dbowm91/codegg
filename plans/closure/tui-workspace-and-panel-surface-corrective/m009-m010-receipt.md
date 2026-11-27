# M009–M010 live TUI receipt

Environment: isolated session `m9m10`, temporary repository `/tmp/codegg-receipt-m9m10/repo`, `HOME=/tmp/codegg-receipt-m9m10/home`, `CODEGG_DATA_HOME=/tmp/codegg-receipt-m9m10/data`. Ran the prebuilt `target/debug/codegg`; no build or product-code edits.

## Setup / commands

Created a clean temporary Git repo (branch `main`, one `README.md` commit), then launched with:

```sh
rm -rf /tmp/codegg-receipt-m9m10
mkdir -p /tmp/codegg-receipt-m9m10/repo /tmp/codegg-receipt-m9m10/home
cd /tmp/codegg-receipt-m9m10/repo && git init -q -b main
printf 'x\n' > README.md && git add .
git -c user.email=r@example.com -c user.name=r commit -qm init
tui-test --session m9m10 run --cwd /tmp/codegg-receipt-m9m10/repo --cols 130 --rows 40 \
  --env HOME=/tmp/codegg-receipt-m9m10/home \
  --env CODEGG_DATA_HOME=/tmp/codegg-receipt-m9m10/data -- $BIN
tui-test --session m9m10 wait idle --timeout 15000
tui-test --session m9m10 text
```

Initial screen showed the isolated cwd, but also an error: `Runtime policy: Policy preference read failed (run_store_unavailable): the command's workspace could not be resolved, so its run cannot be recorded`. The same `run_store_unavailable` later affected the Tasks view. The temp repo itself verified clean on branch `main` and `git rev-parse --show-toplevel` returned `/private/tmp/codegg-receipt-m9m10/repo`.

## A. M009 Workspace picker — PASS

Commands: `/panels` → `i` → type `Workspace` → Enter. The manager filtered to `1/33` with the row `▸ Workspace projects · switch`. Loaded panel text showed the project row `>repo` and footer `⇧Q leave · WORKSPACE · j/k move · Enter open · i filter · Shift+← back`. This is a functioning picker surface, not a placeholder.

Host cycling check: from Workspace, `Shift+J` switched to Help; from Help, `Shift+K` returned to Workspace. Exact observed path: `Workspace --Shift+J--> HELP --Shift+K--> WORKSPACE`. This verifies Workspace is reachable via Shift+K in the panel host; it did not show cycling across all panel entries.

Screenshot: `plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m009-workspace-panel.svg`.

## B. M009 Sessions panel — PASS

Commands: `Shift+Q` → `/panels` → `i` → type `Sessions` → Enter. Screen showed `Sessions` with project `repo`, `scope: this workspace`, explicit empty state `(no sessions loaded)`, and controls including `Esc close · ↑/↓ navigate · Enter open`. No placeholder was shown.

Screenshot: `plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m009-sessions-panel.svg`.

## C. M010 `/tasks` task view — FAIL (backend/store error)

Command: `Shift+Q` → submit `/tasks`. View rendered `Project Tasks` with `RUNNING`, `FUTURE / WAITING`, `NEEDS ATTENTION`, and `RECENT`; each showed `(none)`. Footer advertised `j/k move · J/K reorder · Enter open · d detail`. Pressed `j` and `k`; screen remained unchanged because there were no rows. There was no selected-row detail to inspect. The screen also showed `summary unavailable` and `Error: Task list failed: run_store_unavailable: the command's workspace could not be resolved, so its run cannot be recorded` (warning pane: `Task list failed: run_store_unavailable...`). Therefore the displayed `(none)` cannot be confirmed as a healthy empty result.

Screenshot: `plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m010-task-view.svg`.

## D. M010 Ctrl+T overlay — PASS

From the composer, pressed `Ctrl+T` and confirmed the distinct overlay footer `Nothing pending · Ctrl+T close`. It differs from the `/tasks` view. Pressed `Ctrl+T` again and confirmed return to the composer.

Screenshot: `plans/closure/tui-workspace-and-panel-surface-corrective/receipts/m010-task-overlay.svg`.

## E. Disallowed/placeholder-looking surfaces

Across the screens inspected, no visible text said `Nothing loaded` or `Open it from the panel manager`. No wholly empty bordered box was observed. Sessions used the explicit `(no sessions loaded)` state. The task view was bordered but contained its named sections, each marked `(none)`, plus the store error; it was not an otherwise blank box.

## Close / remaining blocker

Closed with `tui-test close --session m9m10` (successful). Remaining blocker: isolated startup/task queries reported `run_store_unavailable`. Task view's empty-looking sections therefore do not establish a healthy empty state. No code changes were made.
