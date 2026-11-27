# TUI Workspace Central Surface Milestone 001 — Route composer and key fall-through

Status: ready for handoff

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

Primary class: capability / invariant

## 1. Objective

Typing chat from /workspace reaches a visible bottom input whose badge names the chat destination, and no printable intended as text is swallowed by tree navigation.

## 2. Why this milestone is ready

No hard dependency is open. The whole-view shell, NOR/INS router, Chat viewer binding, and armed-composer delivery are all closed lines. The defect is reproducible in source (route claims every key; frame paints no composer; Shift+H only blurs) and was observed live.

Depends on: nothing. Consumes the closed whole-view shell, the chat compose-seed and armed-composer delivery, and the panel pass-through contracts.

## 3. Current implementation evidence

Route fast path paints no composer and the footer is the whole vocabulary (app/render.rs route branch into render_workspace_frame); route handler claims every key (commands/workspace_view.rs); Shift+H helper only blurs (commands/workspace_dashboard.rs focus_workspace_composer); chat pass-through and armed delivery exist (panels/chat.rs, app/prompt_turn.rs, commands/chat.rs).

No production code is changed by this plan. Baseline statements above were read at `eefbfa0fa` during the 2026-11-27 audit (source inspection plus isolated-HOME live `tui-test` drive).

## 4. Invariants that must not regress

- The workspace route never opens a centered small-box dialog for a workspace or worktree action.
- A typed printable reaches exactly one visible input that names its destination first.
- Leaving the route restores the session composer untouched; daemon work is never cancelled on route exit.

## 5. Scope

### In scope

- Paint the route-owned bottom input on render_workspace_frame with a destination badge for the armed chat target.
- Restructure handle_workspace_view_key into navigation-owned keys plus explicit decline of composition keys to the panel host then the route composer.
- Make Shift+H focus the route composer; keep Shift+X/Esc exit semantics exactly as today.
- Update footer, title-row, and hint vocabulary in the same change.

### Explicitly out of scope

- Forcing Chat docked Right (M002).
- Reveal content, member fetch, add-member (M003).
- Lifecycle/launch rendering moves (M004).
- /worktree navigation (M005).
- Any daemon, protocol, storage, scheduler, or desktop change.

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

### Work package A — A — Route composer binding and paint

A — Route composer binding and paint: add the route-owned composer binding with focus accounting (route composer vs panel vs filter); paint it in the workspace frame with destination badge; acceptance: badge names armed chat before typing, session composer byte-identical after exit.

### Work package B — B — Key fall-through

B — Key fall-through: route declines composition keys; tree verbs keep their modes; acceptance: typed text lands exactly once in the route input, tree verbs still work in NOR, filter still works in INS.

### Work package C — C — Focus and exit

C — Focus and exit: Shift+H focuses route composer; Shift+L still reaches chat; Esc/Shift+X paths unchanged; acceptance: focus transitions are badge-visible and reversible.


## 8. Failure, cancellation, restart, and contention semantics

Leaving the route cancels frontend workspace tasks only; daemon work is never cancelled. Late completions drop against generation/selection. Retried submits reuse the idempotency key; validation refusals start a new one. Restart restores no route state beyond a fresh open.

## 9. Compatibility and migration

No wire, storage, or catalog break. Older peers unaffected (no protocol change). If a catalog spelling moves, daemon catalog and TUI set move together with the ownership guard as proof. `HTTP /api/projects*` untouched.

## 10. Required tests

- Focused: composer badge and routing unit tests; key-router tests proving composition keys are declined while navigation keys are owned; focus/exit tests.
- Integration: tui-test receipt typing chat from the route with the badge visible and delivery confirmed (isolated HOME/DATA_HOME, remote-backed repo).
- Negative: tree verb keys do not produce text; text keys do not move the tree in the same mode.

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

- Printable typed with chat focus lands once in the visible route input; badge names destination first.
- No key is both tree verb and text in one mode.
- Exit restores the session composer; no daemon work cancelled.
- Live receipt captured with recorded size/cwd/waits.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Frame cannot host input without breaking host hit-testing/focus — stop and record the geometry limit.
- A printable must be both verb and text in one mode — stop, modes are wrong.
- Any required protocol/storage change appears — stop and re-plan.
- scope would expand into another subsystem roadmap.

## 15. Closure evidence required

- Route composer + fall-through tests green; tui-test chat-typing receipt with badge and delivery; guards green; exact commands recorded.
- Closure record at `plans/closure/tui-workspace-central-surface-corrective/001-status.md` with requirement-to-evidence matrix, exact commands, and receipt paths.

## 16. Handoff notes

- Isolated HOME/DATA_HOME recipe; remote-backed repo required (local-only identity is a separate open decision).
- Baseline `eefbfa0fa`, branch `feat/auto-20261126-cae46b00`; rebase the baseline check at handoff.
- No code changes were made while writing this plan; the tree state the plan was written against is the commit above.
