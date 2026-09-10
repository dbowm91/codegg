# Multi-Project TUI Frontend Convergence M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-project-sessions/005-project-execution-context-command-scope.md`

Source subsystem roadmap:

- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md#12-milestone-status`

Repository baseline reviewed: `98bc89f`.

Implementation commits:

- `4a963e0` — explicit project execution context, active-tab command catalogs,
  scoped shell/test/plugin/agent and project-sensitive TUI operations,
  canonical restore roots, guard integration, tests, and documentation.

Closure commit:

- This commit — closure evidence, roadmap/registry status, and downstream
  dependency disposition.

## 1. Executive finding

M005 is complete. Project-scoped TUI work now resolves a small immutable
`ProjectExecutionContext` from the active tab and captures it before any
background task is spawned. It carries project/workspace/session identifiers
and the canonical workspace-root locator. Project-local commands are owned by
the `App` instance and discovered from that explicit root; the compatibility
`COMMAND_REGISTRY` contains only built-ins/global definitions.

The implementation preserves daemon authorization, scheduler ownership,
existing command precedence, and the legacy bootstrap path. It does not add a
daemon, protocol, storage, or identity owner.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Explicit active execution context | `src/tui/app/state/execution_context.rs`; `App::project_execution_context()` | pass |
| Fail closed without an active/rooted tab | resolver unit tests for no active tab and missing root; actionable toast paths | pass |
| No ambient TUI project authority | census below; no `session_state.project_dir` or `current_dir()` in scanned TUI files | pass |
| Shell captures the selected root | `TuiCommand::RunHumanShell { cwd }`; shell confirmation preserves the captured root | pass |
| `/test` captures root before spawning | `start_test_run()` resolves context before request/task creation and passes the root to `build_test_request` | pass |
| Process project command/plugin scope | `start_plugin_command()` captures root; runtime cwd and `PluginContext.project_dir` are explicit | pass |
| Project-local command discovery | `CommandRegistry::new_for_workspace_root()` and `App::refresh_project_command_registry()` | pass |
| A→B→A catalog behavior without chdir | command registry and active-tab switching regression tests | pass |
| Restore active B before heavy session load | restore detail snapshots carry workspace-id→canonical-root mappings into `ProjectTabState` | pass |
| Scoped process command directories | `resolve_scoped_cwd()` rejects paths outside the canonical root; negative test included | pass |
| Asset/tab invalidation | registry refresh on active-tab switch, restore application, and asset refresh completion | pass |
| Guard invocation | `scripts/verify.sh quick` and the existing CI `verify` job invoke `check_tui_project_authority.py` | pass |
| No new persistence/protocol owner | workspace root is live tab routing data; manifest schema and daemon protocol remain unchanged | pass |

## 3. Ambient-context census and disposition

The implementation census covered the plan's required files and the adjacent
TUI session, memory, research, Git/worktree, agent, asset, and event paths.

| Candidate | Disposition |
|---|---|
| `src/tui/command.rs` dynamic discovery | Migrated to `new_for_workspace_root(&Path)`; built-ins remain available through the compatibility static. |
| `src/tui/components/dialogs/command.rs` global palette | Palette now owns cloned commands and accepts the active App registry; it is refreshed on scope changes. |
| `src/tui/commands/shell.rs` human shell cwd | Resolved once from the active context and carried through confirmation/re-run/dispatch. |
| `src/tui/commands/test.rs` `/test` workdir | Resolved once before spawning; the request and daemon workspace registration use that captured root. |
| `src/tui/commands/plugins.rs` process command cwd/context | Captured root supplies runtime cwd and plugin context; configured subdirectories are root-contained. |
| `src/tui/commands/agents.rs` registry helpers | Production list/show/diff/validate/rebuild paths take an explicit root; test compatibility wrappers are isolated. |
| App session, memory, research, task, event, Git/worktree and security paths | Replaced ambient `project_dir`/cwd selection with active-root/project-key accessors or explicit context capture. Presentation-only session directory rendering remains display data. |
| Project picker and restore | Daemon `ProjectGet` workspace `canonical_root` is stored on the tab; restored tabs do not infer a root from launch cwd. |
| Compatibility startup | `ProjectTabs::from_compat()` captures/canonicalizes the initial directory once at composition/bootstrap; it is not reread as mutable project state. |

The final scan found no `std::env::current_dir()` or direct
`session_state.project_dir` reads under the protected TUI execution surface.
The guard's allowlist now accepts only doc/test lines and explicitly marked
bootstrap/test-fixture lines; it no longer accepts syntactic forms such as
`let cwd = current_dir()`.

## 4. Production implementation evidence

- `ProjectTabState.workspace_root` stores the canonical locator separately
  from `ProjectId`, `WorkspaceId`, and `SessionId`.
- `ProjectExecutionContext` is resolved at the dispatch boundary and moved
  into asynchronous work rather than rereading mutable active state.
- Human shell, supervised tests, process-backed project commands, agents,
  memory, research, worktree/task, security review, and related local helpers
  use active-tab scope.
- Restored workspace roots are derived from existing `ProjectDetailsDto`
  data. Invalid/stale workspace bindings remain unrooted and fail closed.
- `CommandPalette` no longer stores references into a global registry, so a
  tab switch cannot leave startup-project commands visible.
- `resolve_scoped_cwd()` preserves relative command-directory behavior under
  the active root while rejecting absolute or traversal paths outside it.
- No secrets, credentials, prompt content, command bodies, or durable command
  metadata were added to the context or manifest.

## 5. Verification executed

Passed:

- `cargo fmt --all -- --check`
- `cargo check -p codegg --tests`
- `python3 scripts/check_tui_project_authority.py`
- `scripts/verify.sh quick`
- `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `git diff --check`

`cargo check -p codegg --tests` compiled the focused unit and integration test
targets, including `tui_project_tabs`, `tui_project_routing`, and
`tui_manifest_restore`. The representative focused test invocation
`CARGO_BUILD_JOBS=1 cargo test -p codegg --lib -- tui::app::state --no-fail-fast`
was attempted but could not link on this host: the x86_64-apple-darwin Rust
toolchain selected arm64 `/opt/local` `liblzma`/`libiconv` libraries, producing
undefined x86_64 lzma symbols. This is a host toolchain/linker limitation,
not a Rust compilation or test assertion failure; the source/test-target
compile and all static/broad checks passed.

## 6. Invariant, failure, and security review

- Durable IDs retain their existing semantics; roots remain locators and are
  never promoted to project identity.
- No tab switch changes process cwd. An unresolved root produces an actionable
  error instead of silently selecting the launch directory.
- Background operations retain the root selected at dispatch. Existing request,
  route, and tab-generation handling continues to govern UI completion.
- Restore uses daemon-provided workspace roots and does not convert stale
  manifest data or launch cwd into authority.
- Process project command directories are canonicalized and root-contained;
  plugin permissions, environment policy, and daemon checks remain unchanged.
- The context contains no secrets and is not an authorization grant.

## 7. Compatibility and documentation

Built-in/global command names and collision precedence remain stable. Project
command files are still discovered through the existing asset format and
refresh seams. Standalone/CLI startup still captures its initial directory at
the compatibility boundary. Updated documentation:

- `architecture/tui.md`
- `architecture/command.md`
- `.opencode/skills/tui/SKILL.md`
- `scripts/check_tui_project_authority.py`

## 8. Roadmap disposition and unblocking audit

M005 is closed in the corrective addendum and registry. The dependency audit
found:

- M006 is now `ready for handoff` (hard M005 dependency satisfied).
- M010 is now `ready for handoff`; M009 remains a soft sequencing preference.
- M007 remains independently `ready for handoff`.
- M008 remains blocked by M006 and M007 (and the already-closed M005).
- M009 remains blocked by M007.

The following status files and registry entries were updated in the closure
commit:

- `plans/implementation/tui-project-sessions/005-project-execution-context-command-scope.md` → `closed`
- `plans/implementation/tui-project-sessions/006-nonblocking-session-submit-lifecycle.md` → `ready for handoff`
- `plans/implementation/tui-project-sessions/010-command-discovery-keybinding-convergence.md` → `ready for handoff`
- `plans/subsystems/tui-project-sessions-frontend-convergence-corrective-addendum.md`
- `plans/registry.md`

No future plan was unblocked beyond M006 and M010; M008 and M009 retain their
real remaining hard dependencies.

## 9. Final recommendation

Closed. No corrective pass is required for M005. The only outstanding local
verification action is to rerun the focused binaries on a compatible
host/toolchain or repair the host's cross-architecture library selection.
