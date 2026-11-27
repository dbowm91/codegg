# TUI Workspace Central Surface Milestone 004 — Lifecycle and launch as main-panel modes

Status: proposed

Repository baseline: `eefbfa0fa3b135fc1af3fb2ed9c1d94d1f8cf643` (`feat/auto-20261126-cae46b00`, worktree clean at plan time)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#10-project-catalog-and-discovery`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#25-tui-target-behavior`

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md`
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`
- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`

Primary class: capability

## 1. Objective

Add/edit/archive/restore/destroy/team-admin/launch render as full-size main-panel modes; no /workspace-originated small box remains.

## 2. Why this milestone is ready

Builds on M003's selection guard. Request functions and state machines exist; only their rendering moves (picker modal + footer line into main body).

Hard dependency: M003 closed (lifecycle/team completions share the row-scoped selection guard).

## 3. Current implementation evidence

a/e/s/x via begin_lifecycle_from into picker phases (project_picker.rs); launch sheet state machine + footer rendering (state/workspace_view.rs, render.rs); token secret stays modal (team.rs DeviceSecretDialog).

No production code is changed by this plan. Baseline statements above were read at `eefbfa0fa` during the 2026-11-27 audit (source inspection plus isolated-HOME live `tui-test` drive).

## 4. Invariants that must not regress

- No route-originated Project Picker / Team / Worktrees box; global picker hotkeys keep working outside the route.
- Destroy names session count; archive arms-then-performs; token plaintext never enters panel state.
- TUI never mints paths/branches; one idempotency key per submit.

## 5. Scope

### In scope

- View-owned Register/Edit/State/Destroy/Team modes with full-width forms + confirmations; launch placement/base/branch as main-body flow; remove route callers of begin_lifecycle_from and InfoType::Team/WorktreeList openers on this path.

### Explicitly out of scope

- /worktree navigation (M005).
- Membership semantics, launch authority, or secret-display changes.

## 6. Required production changes

Likely modules (mechanics may adjust; contracts may not): `crates/codegg-tui/src/commands/workspace_view.rs`, `crates/codegg-tui/src/commands/workspace_dashboard.rs`, `crates/codegg-tui/src/app/render.rs` (workspace frame), `crates/codegg-tui/src/app/mod.rs` (route dispatch order), `crates/codegg-tui/src/panels/chat.rs` + `crates/codegg-tui/src/chat_browser.rs` (composition seams), and — per milestone — `crates/codegg-tui/src/commands/team.rs`, `crates/codegg-tui/src/commands/project_picker.rs`, `crates/codegg-tui/src/commands/tasks.rs`, `crates/codegg-tui/src/components/dialogs/`. No daemon, protocol, storage, scheduler, provider, or desktop change. Docs (`architecture/tui.md`, `docs/tui.md`, help overlay) move in the same change as the behavior.

### Core/domain

No change (projection only).

### Storage and migrations

None. Stop and re-plan if a durable field appears necessary.

### Protocol and DTOs

None. Reuse existing requests; no new field or variant.

### Runtime and concurrency

Preserve generation/reconnect guards; route exit cancels frontend workspace requests only; late completions drop against stale generations.

### Frontend or operator surface

As scoped above; footer/title/hint vocabulary updated with the behavior; no new global key.

### Security and authorization

Projection only; daemon-authorized effects; team ops stay owner-gated and revision-checked; no secret enters panel state; token secret modal untouched.

### Documentation and static guards

Guards named in §11 stay green in the same change.

## 7. Ordered work packages

### Work package A — A — Lifecycle modes

A — Lifecycle modes: full-panel forms reusing request functions; acceptance: each completes with no centered box.

### Work package B — B — Launch in-panel

B — Launch in-panel: placement/base/branch in main body reusing LaunchSheet machine; acceptance: both placements submit, collision typed, retry converges.

### Work package C — C — Route hygiene

C — Route hygiene: route never opens picker/Team/Worktrees boxes; global picker + secret modal unchanged; acceptance: hotkeys outside route verified.


## 8. Failure, cancellation, restart, and contention semantics

Leaving the route cancels frontend workspace tasks only; daemon work is never cancelled. Late completions drop against generation/selection. Retried submits reuse the idempotency key; validation refusals start a new one. Restart restores no route state beyond a fresh open.

## 9. Compatibility and migration

No wire, storage, or catalog break. Older peers unaffected (no protocol change). If a catalog spelling moves, daemon catalog and TUI set move together with the ownership guard as proof. `HTTP /api/projects*` untouched.

## 10. Required tests

- Each lifecycle mode + confirmations; launch placements incl. collision + idempotent retry; route never mounts a dialog in these flows.
- tui-test receipts per mode with no centered box on screen.

Plus the plan-appropriate focused suites: `cargo test -p codegg-tui --lib`, the touched TUI family, and `scripts/check_test_assertion_quality.py` + `scripts/check_test_duplication.py` for new bodies.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib
# touched family, e.g. cargo test -p codegg-tui --test tui_render -- <member>
python3 scripts/check_command_catalog_ownership.py
cargo fmt --check && git diff --check
scripts/verify.sh scoped
scripts/verify.sh quick
```

Do not claim commands not actually run in the closure record. `tui-test` receipts use isolated HOME + isolated DATA_HOME against a remote-backed repo with recorded size/cwd/waits.

## 12. Documentation updates

- `architecture/tui.md`, `docs/tui.md`, help overlay, command descriptions — same change as behavior.

## 13. Acceptance criteria

- All actions complete in main panel; counts/confirmations/denials truthful; secret modal unchanged; live receipts.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- An action has no daemon request — view says so and stops, no synthesis.
- A new launcher surface appears — stop and update writer disposition first.
- scope would expand into another subsystem roadmap.

## 15. Closure evidence required

- Lifecycle + launch tests green; receipts; guards green incl. catalog ownership.
- Closure record at `plans/closure/tui-workspace-central-surface-corrective/004-status.md` with requirement-to-evidence matrix, exact commands, and receipt paths.

## 16. Handoff notes

- Largest UI surface; keep picker request functions shared, rendering moved.
- Baseline `eefbfa0fa`, branch `feat/auto-20261126-cae46b00`; rebase the baseline check at handoff.
- No code changes were made while writing this plan; the tree state the plan was written against is the commit above.
