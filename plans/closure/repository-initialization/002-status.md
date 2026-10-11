# Repository Initialization Milestone 002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/repository-initialization/002-init-command-preview-publish-refresh.md`
Source subsystem roadmap: `plans/subsystems/repository-initialization-roadmap.md#7-milestones`
Repository baseline reviewed: `a449460` (M001 closed)
Implementation commits:
- `3f8ecc48c1a955cde78f1f30140f10411b4d3364` — guarded `/init` workflow, daemon protocol, auth, UI, docs
- `b28f2cc076d10e5c829860580728291fc5a58604` — no-follow target reads against symlink swaps
- `0c06c5e97e0566f47652de60456128bdb30797ff` — TUI/CoreClient/daemon lifecycle integration tests
- `065dab05e9e5c37c4e3c5eb5a9b2019093114fa6` — stale-project approval rejection test

## 1. Executive finding

M002 delivers a real TUI `/init` command for the selected project. It requests the deterministic M001 proposal through CoreClient, shows the full candidate, diff, evidence, and diagnostics, and requires a deliberate `a` key action to publish. Esc cancels. Publication is a daemon-owned, direct-project `file.modify` operation using a client/project/workspace-bound, one-use token and the fixed workspace-root `AGENTS.md` path. A successful publication triggers a scope-explicit runtime asset refresh. Existing in-flight turns retain their pinned snapshot.

ADR-0014's accepted token-only detail was preserved as history and superseded by accepted ADR-0015 after implementation evidence confirmed the existing authorization preamble requires explicit project locators.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Canonical executable `/init` registration and exhaustive routing | `BuiltinSlashAction::ProjectInit`, command catalog test, and exhaustive app dispatcher; docs count updated to 155 commands / 148 actions |
| Bounded preview with candidate, readable diff, evidence, and diagnostics | Core draft request calls `analyze_project_context`; DTO size bounded; scrollable ProjectInit info dialog |
| Explicit consent independent of agent approval mode | TUI integration test exercises preview, cancel, stale-project refusal, then explicit `ProjectInitApprove` publication |
| Selected project/workspace remains bound | Publish carries project/workspace IDs; daemon token is bound to authenticated client and canonical root; TUI rejects approval after a project switch |
| Daemon authorization and audit | `project_init_draft` maps to direct-project `file.read`; `project_init_publish` maps to direct-project `file.modify` and `file_mutate`; authorization and audit coverage guards pass |
| Create, update, and preservation of user text | CoreDaemon request trajectory creates `AGENTS.md`, then updates it after evidence changes while retaining a hand-written note |
| Stale target and symlink handling | Stale digest and symlink publication tests; Unix `O_NOFOLLOW` and Windows open-reparse-point flags on preview reread; update writes use a same-directory temporary plus atomic replacement |
| Competing create and one-use/restart semantics | Exactly-one-winner concurrent create test; four registry tests cover scope, single use, expiry, and loss of drafts after daemon restart |
| Successful scoped refresh and pinned active turns | In-process TUI→CoreClient→daemon integration receives `AssetRefreshFinished`; `publishes_generation_and_pins_previous_snapshot` passes |
| No provider or command execution required | TUI integration uses CoreClient only; M001 analyzer remains deterministic, read-only, and provider-independent |

## 3. Production implementation evidence

`src/tui/command.rs`, `src/tui/app/input.rs`, `src/tui/runtime/command_dispatch.rs`, and `src/tui/commands/project_init.rs` implement command dispatch, preview, approve/cancel, task lifecycle, and scoped refresh. The preview response is stored only in TUI dialog state. A changed active project/workspace discards the local draft before a publish request can be queued.

`src/core/project_init.rs` owns ephemeral bounded draft tokens. `src/core/daemon_project_init.rs` resolves the authoritative catalog relation and canonical workspace, invokes the M001 analyzer, rechecks digest and regular-file type, refuses unsaved editor buffers, serializes with the workspace repository lock, and atomically publishes only root `AGENTS.md`. Target reads use no-follow opens on Unix and Windows. File mutation is audited through the established file-mutation action. The protocol adds two requests and two responses; no storage migration was added.

## 4. Verification executed

- `rtk cargo test --lib tui::command --locked` — **231 passed**.
- `rtk cargo test --lib tui::components::dialogs::info::tests --locked` — **1 passed**.
- `rtk cargo test --lib core::project_init::tests --locked` — **4 passed**.
- `rtk cargo test --lib core::daemon_project_init::tests --locked` — **3 passed**.
- `rtk cargo test --lib project_init_daemon_draft_requires_publish_and_rejects_stale_target --locked` — **1 passed**.
- `rtk cargo test --lib tui_approval_publishes_through_core_client_and_daemon --locked` — **1 passed**, including cancel, stale project switch, approved create, and refresh.
- `rtk cargo test --lib agent::asset_refresh::tests::publishes_generation_and_pins_previous_snapshot --locked` — **1 passed**.
- M001 `agent::bootstrap` focused tests — **9 passed** (recorded in `001-status.md`).
- `rtk cargo fmt --all -- --check` — passed.
- `rtk cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `rtk bash scripts/verify.sh quick` — passed, including repository guards and workspace all-target check.
- `rtk cargo check --features server --locked` — passed with one pre-existing deprecation warning in `src/server/ws.rs:958`.
- `rtk python3 scripts/check_authorization_matrix.py` and `rtk python3 scripts/check_audit_coverage.py` — passed.
- `rtk git diff --check` — passed.

No full workspace test suite, hosted CI, or native Windows/macOS runtime test was run. The server-feature check was on Linux.

## 5. Invariant review

The analyzer's root and the daemon's resolved workspace are explicit. The protocol accepts no client-selected path or content. Only root `AGENTS.md` can be published. Model/tool execution and global personal instructions are not involved. Approval is an explicit TUI action and never inferred from automatic or yolo agent policy. Refresh uses the exact published project and workspace scope; snapshot pinning remains unchanged.

## 6. Failure, cancellation, and recovery

Preview and cancel leave the file untouched. Project switch between preview and approval queues no publish. A stale digest, invalid target type, symlink, oversize target, editor dirty-buffer conflict, expired/replayed token, or authorization refusal returns a visible error. Concurrent absent-file creation has one winner. Draft tokens are process-local and lost on daemon restart; the user regenerates the preview. A duplicate publish cannot reuse a consumed token.

## 7. Documentation and architecture updates

Updated `docs/tui.md`, `docs/repository-init.md`, `architecture/agent.md`, `architecture/command.md`, `architecture/core.md`, `architecture/authorization.md`, `architecture/protocol.md`, `architecture/overview.md`, and the agent/core/authorization skills. Protocol, command, and authorization counts were checked against their source. ADR-0014 is marked superseded and ADR-0015 records the direct-project locator detail.

## 8. Unresolved findings

No known medium- or high-severity implementation finding remains. The two-second analyzer budget remains cooperative between bounded filesystem operations as documented by M001. Native non-Linux behavior and hosted CI remain unqualified.

## 9. Roadmap and registry disposition

M002 is closed. The `/init` roadmap is complete with M001 and M002 closed; there are no downstream milestones in this line to unblock. Registry and roadmap status are updated in the closure commit.
