# TUI Workspace Central Surface Milestone 002 — Force Chat docked Right on entry

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

/workspace entry always presents Chat docked Right, visible, and bound to the selection — even when the Right edge was previously closed.

## 2. Why this milestone is ready

Builds on M001's composer and fall-through. Panel host dock/visibility/bind contracts are closed lines.

Hard dependency: M001 closed (forced chat without the typing path repeats the defect).

## 3. Current implementation evidence

Entry selects Chat then blurs (open_workspace_dashboard); closed Right edge renders the unavailable fallback (render workspace frame chat branch); selection rebind via sync_workspace_chat_panel.

No production code is changed by this plan. Baseline statements above were read at `eefbfa0fa` during the 2026-11-27 audit (source inspection plus isolated-HOME live `tui-test` drive).

## 4. Invariants that must not regress

- Right edge shows bound Chat after entry; the unavailable fallback is unreachable from this route.
- Selection changes rebind; no second chat cache; no placement persistence change.

## 5. Scope

### In scope

- Dock Chat Right + visible + bound on entry; rebind on selection change through the existing sync path.
- Remove the route-reachable unavailable fallback branch.

### Explicitly out of scope

- Reveal content (M003).
- Lifecycle/launch modes (M004).
- /worktree navigation (M005).
- Any new panel kind or host change.

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

### Work package A — A — Entry docking

A — Entry docking: dock+show+bind on open; acceptance: closed-Right entry paints bound Chat with no manager detour.

### Work package B — B — Rebind

B — Rebind: selection changes rebind and refresh only when stale; acceptance: switching rows rebinds, per-project drafts preserved.


## 8. Failure, cancellation, restart, and contention semantics

Leaving the route cancels frontend workspace tasks only; daemon work is never cancelled. Late completions drop against generation/selection. Retried submits reuse the idempotency key; validation refusals start a new one. Restart restores no route state beyond a fresh open.

## 9. Compatibility and migration

No wire, storage, or catalog break. Older peers unaffected (no protocol change). If a catalog spelling moves, daemon catalog and TUI set move together with the ownership guard as proof. `HTTP /api/projects*` untouched.

## 10. Required tests

- Entry from closed-Right paints bound Chat; selection switch rebinds; no unavailable text on the route.
- tui-test receipt: close Right, open /workspace, Chat visible and bound.

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

- Closed-Right entry paints bound Chat.
- Rebind on selection; drafts preserved.
- Live receipt.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Host cannot dock without a placement change — stop, do not invent a panel kind.
- scope would expand into another subsystem roadmap.

## 15. Closure evidence required

- Dock/bind tests green; tui-test receipt; guards green.
- Closure record at `plans/closure/tui-workspace-central-surface-corrective/002-status.md` with requirement-to-evidence matrix, exact commands, and receipt paths.

## 16. Handoff notes

- Same isolated receipt recipe as M001.
- Baseline `eefbfa0fa`, branch `feat/auto-20261126-cae46b00`; rebase the baseline check at handoff.
- No code changes were made while writing this plan; the tree state the plan was written against is the commit above.
