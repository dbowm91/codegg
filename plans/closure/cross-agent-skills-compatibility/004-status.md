# Cross-Agent Skill Compatibility M004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/cross-agent-skills-compatibility/004-scoped-discovery-and-inspection.md`

Source roadmap: `plans/subsystems/cross-agent-skills-compatibility-roadmap.md`

Repository baseline reviewed: `11e18b0797b2290f7e1d7c0f12c301466e98c03f`

Implementation commits: `fb63817439f9bd470c01e61f84cd49193f9bca86`,
`bfa5bcd001f297500e6cf6c36bcba984c0212f04`, and
`9f55a80c5c15208a25e0705f67b2c4ceaaa331d2`.

## 1. Executive finding

M004 is closed. Skill discovery walks only the selected directory and eligible
ancestors through the nearest `.git` directory or worktree `.git` file, with an
eight-level bound. Nearest scope wins same-name conflicts; siblings and paths
outside the Git boundary are excluded. `/skills [name]` reads the active
immutable snapshot and reports bounded effective/shadowed provenance; `/reload`
continues to refresh later turns without mutating in-flight snapshots.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Git boundary, nearest scope, no-Git fallback, sibling isolation | registry ancestor-scope unit fixtures | pass |
| Worktree `.git` file and `.git` directory | `worktree_gitfile_marks_the_ancestor_boundary`; `ancestor_scopes_are_bounded_and_nearer_scopes_win` | pass |
| Project depth and deterministic precedence | `EffectiveSkill.workspace_depth`; root/source ordering and nested-scope fixtures | pass |
| Real read-only `/skills` dispatch response | `skills_command_uses_snapshot_path_and_reports_unavailable_snapshot` | pass |
| Palette registration and discoverability | `skills_inspection_command_is_registered_as_a_read_only_builtin` | pass |
| Captured digest remains pinned | `pinned_skill_activation_records_the_captured_digest` | pass |

## 3. Verification executed

- `rtk cargo test -p codegg --lib skills:: --locked` — 48 passed.
- `rtk cargo test --test skills_registry --locked` — 29 passed.
- `rtk cargo test --lib tui::app::skills_command_tests --locked` — 1 passed.
- `rtk cargo test --test tui skills_inspection_command_is_registered_as_a_read_only_builtin --locked` — 1 passed.
- `rtk cargo test --test asset_snapshot pinned_skill_activation_records_the_captured_digest --locked` — 1 passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `rtk bash scripts/verify.sh quick` — passed.

## 4. Security and concurrency review

No renderer or palette path scans files. The command uses only the current
snapshot, returns no physical paths or skill bodies, caps query and toast
output, and resets prompt state. Existing refresh and active-turn pinning
semantics are unchanged. The bounded ancestor walk never scans descendants or
siblings.

## 5. Findings and handoff

No blocking findings remain. Native macOS/Windows worktree fixtures were not
run. M005's hard dependency is discharged; its closure is recorded separately
at `005-status.md`.
