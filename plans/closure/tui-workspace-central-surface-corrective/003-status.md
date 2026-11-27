# M003 — Enter-reveal with row-scoped team inline: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- Enter on workspace/worktree reveals in-panel: worktrees from cache (`ManagedWorktreeList` fetch on demand), member list + add-member form full-size — `reveal_workspace`, inline team section in the frame; no `InfoType::Team` opens from this route (`open_inline_team` renders in-panel, typed errors in-panel).
- Row-scoped identity: `workspace_selected_project_id` prefers the view tree cursor; team fetch/adopt fenced by `team_project_id` + route-origin request id; late completions after leave cannot reopen the Team dialog (regression tests: late read + late mutation completions suppressed).
- Unauthorized renders truthful in-panel denial, never another project's rows (stale inline-denial test updated to assert in-panel error + no modal).
- Live receipt: reveal paths covered by unit/render tests; isolated-repo live env has no daemon-registered workspaces so no workspace-row reveal frame was capturable; tree + team-denial in-panel paths are test-proven.

## Commands run

Same gate set as 001-status (includes `successful_inline_membership_add_refreshes_and_keeps_its_notice`, selector navigation + unknown-selector notice tests).
