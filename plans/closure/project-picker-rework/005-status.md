# Project Picker Rework Milestone 005 — Closure Status

Status: conditionally closed
Source implementation plan: `plans/implementation/project-picker-rework/005-directory-identity-and-client-cwd.md`
Source subsystem roadmap: `plans/subsystems/project-picker-rework-roadmap.md#7-milestones`
Repository baseline reviewed: `45100903` (M004 closure)
Implementation commits: `a699b56f` — enforce directory identity and client cwd

## 1. Executive finding

M005's production behavior and local acceptance criteria are implemented.
New canonical directory roots receive distinct projects, reopening a root
returns its existing binding, historical multi-root projects remain readable,
and startup plus picker opens share the same ensure-bound flow. Execution roots
remain on client tabs; the daemon cwd guard passes.

The plan is conditionally closed because its installed-binary, daemon-restart
walkthrough was not performed in this environment. That walkthrough must show
opening and switching among multiple directories, status-line cwd changes, and
identity preservation after daemon restart before the roadmap can be strictly
closed. Hosted CI was also not run locally.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Finding |
|---|---|---|
| Distinct roots have distinct project identities, including shared Git lineage | Project catalog integration and `ProjectStorage` same-lineage tests | Satisfied |
| Reopening a root and concurrent first opens coalesce | Concurrent first-registration integration test; concurrent workspace reconciliation test; launch A→B→A test | Satisfied |
| Historical multi-root projects remain readable and do not gain a new root | `grandfathered_multi_root_project_is_readable_but_not_extended` | Satisfied |
| Explicit association cannot add a second root to a project | `explicit_rebind_refuses_to_add_a_second_workspace_to_a_project` | Satisfied |
| Launch and picker use one directory ensure-bound path | Both call `ensure_bound_project_context`; launch A→B→A integration test | Satisfied |
| Active tab owns the workdir and a dispatched context cannot leak after switching | `captured_execution_context_is_isolated_from_later_tab_switches`; existing active-tab command-root tests | Satisfied |
| Daemon cwd remains unchanged | `python3 scripts/check_daemon_cwd_usage.py` | Satisfied |
| Restart/resume preserves binding and live status-line context | Durable binding and existing manifest restore coverage; installed-binary restart walkthrough not run | Operational evidence outstanding |

## 3. Production implementation evidence

- Added `ensure_bound_project_context`, shared by launch bootstrap and the
  local project picker. It canonicalizes the frontend-owned directory,
  registers the workspace, and registers or loads a project named after the
  directory.
- Changed catalog registration and workspace reconciliation so repository
  lineage is shared metadata rather than project identity. A previously
  unseen root gets its own project; a bound root resolves to its existing
  project. Concurrent first registrations discard the losing speculative
  project and return the winning binding.
- Changed conservative legacy association to create a directory-owned
  project while reusing repository metadata. Explicit workspace association
  refuses to add another root to a project that already owns a different
  root. M004 rebind continues to consolidate historical multi-root bindings.
- Project selection commits the canonical root and typed IDs onto the tab.
  `ExecutionContext` remains captured at dispatch from that active tab; no
  daemon `chdir` or schema migration was added.
- Updated user and architecture documentation and the TUI skill with the
  one-to-one rule, grandfather behavior, shared open flow, and client cwd
  authority.

## 4. Verification executed

All commands were run locally. This host's default Homebrew compiler targets
x86_64 while its native libraries are arm64, so Rust test commands used the
installed arm64 Rust 1.98.1 toolchain explicitly.

- `cargo test --target aarch64-apple-darwin -p codegg-core -p codegg --lib project`
  — 496 root tests and 124 core tests passed.
- `cargo test --target aarch64-apple-darwin -p codegg --lib tui::` — 1,710
  tests passed.
- `cargo test --target aarch64-apple-darwin -p codegg-core --test project_catalog`
  — 25 tests passed; the subsequently added grandfather compatibility test
  passed separately.
- Focused explicit-association refusal test — 1 passed.
- `scripts/verify.sh quick` — passed, including formatting, repository guards,
  and `cargo check --workspace --all-targets --locked`.
- `python3 scripts/check_daemon_cwd_usage.py` — passed.
- Hosted CI and an installed-binary interactive daemon-restart walkthrough
  were not run.

## 5. Invariant review

- New bindings follow one canonical directory to one project. Repository
  lineage does not merge directories.
- Historical multi-root records remain readable. M004 rebind consolidates
  them; explicit association cannot extend them with another root.
- Client tabs own workspace roots. Existing context capture prevents a later
  tab switch from retargeting in-flight work.
- No daemon process cwd mutation was introduced; the static cwd guard passed.
- Tab close remains independent from durable project/session lifecycle.

## 6. Failure and recovery review

Workspace and project registration remain idempotent. Concurrent first-open
registration coalesces on the workspace binding and rolls back a losing
speculative project. Reopen after process restart uses the persisted workspace
binding. Existing manifest restore retains tab workspace roots. A live daemon
restart and visible status-line check remain the operational condition listed
above.

## 7. Migration and compatibility review

No schema migration was required. Existing multi-root bindings are not rewritten
automatically and remain queryable. Operator rebind through M004 selects one
root and removes the old extra bindings. Repository records remain reusable
across roots.

## 8. Security review

The picker remains local-transport-only for raw directory selection. The
frontend canonicalizes the chosen directory before sending it through the
existing workspace-registration authorization path. Existing registration
authorization is unchanged. The daemon cwd usage guard passed.

## 9. Documentation and operations

Updated `docs/tui.md`, `architecture/workspace.md`,
`architecture/project_identity_storage.md`, `architecture/project_catalog.md`,
`architecture/identity.md`, and `.opencode/skills/tui/SKILL.md`. The required
post-install, daemon-restart, multi-directory walkthrough remains pending and
is the sole operational condition for strict roadmap closure.

## 10. Unresolved findings

- **Low — operational evidence:** run the installed-binary multi-directory
  open/switch flow after restarting the daemon and confirm status-line cwd and
  A→B→A project identity. No production correctness finding remains.

## 11. Roadmap disposition

M1–M5 production scopes are implemented. M005's automated and static local
acceptance is satisfied; strict roadmap closure waits only for the operational
walkthrough. Multi-cwd splits remain explicitly deferred.

## 12. Registry updates

- M005 moved from active to conditionally closed; roadmap moved to closing
  pending the operational evidence above.
- Unblock audit: no registered project-picker plan became newly eligible.
  M6 split switching is deferred and has no registered implementation plan.
  No other plan in this line is eligible to continue.
