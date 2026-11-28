# TUI Workspace Central Surface Corrective Milestone 009 — Archive-first lifecycle and launch completions that know their request

Status: ready for handoff

Repository baseline: `256b215ac` on `feat/auto-20261126-cae46b00` (worktree clean; verification performed 2026-11-27)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#13-post-closure-verification-and-corrective-milestones`
- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#m004--lifecycle-and-launch-are-main-panel-modes-the-route-opens-no-small-boxes`

Source closure record this milestone corrects:

- `plans/closure/tui-workspace-central-surface-corrective/004-status.md` — records destroy "reachable through the counted confirmation" and asserts the route never calls `begin_lifecycle_from`. Both claims are false in the shipped code.

Long-term requirements:

- `plans/000-long-term-specification.md#9-project-repository-workspace-and-worktree-model`
- `plans/000-long-term-specification.md#10-project-catalog-and-discovery` — deletion is a catalog authority, not a UI shortcut.
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`

Applicable ADRs:

- `plans/adrs/ADR-0033-directory-tied-project-identity-and-client-working-directory.md` — a launch needs the project's resolved directory binding.
- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md` — session launch is a durable request with an idempotency identity.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no catalog change.

Primary class: **capability** (a lifecycle action either works or explains itself) with an **invariant** half (a retry must not duplicate work, and a stale completion must not land on a newer request).

## 1. Objective

Every lifecycle key on the route either performs its action or gives an actionable typed refusal that names the next step; a launch retry reuses its idempotency key on transport uncertainty and rotates it only after a definitive refusal; and a completion whose `request_id` is not the sheet's current one changes nothing.

## 2. Why this milestone is ready

- The archive-first rule is already authoritative and implemented: `ProjectCatalog::delete_project` refuses an active project with "project must be archived before permanent deletion" (`crates/codegg-core/src/project_catalog.rs:986-1005`), and the picker's destroy loader accepts only a `ProjectGet` whose `archived_at.is_some()` (`crates/codegg-tui/src/commands/project_picker.rs:613-640`).
- The route lists active projects only (`include_archived: false`, `crates/codegg-tui/src/commands/project_catalog.rs:77-85`), so `x` on every row the route can show is guaranteed to fail — measured live: `Error: Project is no longer inactive; refresh and try again`.
- The launch request already carries an identity: the envelope derives its idempotency key from the request (`workspace-launch-{request_id}`) (`crates/codegg-tui/src/commands/workspace_view.rs:595-603`), and both completion variants carry `request_id` (`crates/codegg-tui/src/app/commands.rs:1771-1784`). The dispatcher drops both ids (`crates/codegg-tui/src/runtime/command_dispatch.rs:2147-2165`) and the apply handlers accept none (`workspace_view.rs:1577-1604`).
- The documented retry rule already states the required behaviour: the key is "reused on transport retry and replaced after refusal" (`architecture/tui.md:3308-3314`). `apply_launch_finished` rotates it on every `Some(error)` (`workspace_view.rs:1612-1619`).

## 3. Current implementation evidence

### 3.1 Destroy is unreachable from this surface

`begin_lifecycle_from` starts the destroy loading state in Workspace-inline mode (`project_picker.rs:744-752`, `:781-792`) and the loader then refuses because the project is active. The user sees a transport-shaped error string for a rule the UI could have stated.

### 3.2 The arm and the notice disagree

`LifecycleArm` holds the project id and restore mode (`crates/codegg-tui/src/app/state/workspace_view.rs:127-133`). A non-`s` key clears the arm (`workspace_view.rs:1054-1058`, `:1212-1243`) but the separate `notice` keeps painting `press s again to archive repo`. Reproduced live: the notice survived `j`, `d` and `Esc`. `architecture/tui.md:3331-3335` says "any other key disarms", so the code contradicts the documented rule.

### 3.3 Launch: two independent identity holes

- The key rotates on any error, so a transport failure followed by a retry submits under a **new** key and the daemon cannot deduplicate.
- `request_id` is discarded, so a completion for a closed sheet (or a previous sheet) can apply to whatever sheet is open now.

### 3.4 The refusal path the live drive hit

Submit produced `project_context_required: project has no resolved repository to launch from`, raised by `src/core/daemon_sessions.rs:710-718` when `ProjectStorage::project_repository(project_id)` is `None`. The sheet accepted a submit it could already know would fail.

## 4. Invariants that must not regress

- Archive-first stays the catalog's rule: this milestone changes what the **route says**, never what deletion allows.
- A counted confirmation still precedes any destructive effect, and no action silently substitutes for another (`x` must not archive).
- One in-flight launch per sheet; a retry reuses the key; a refusal rotates it.
- A completion applies only to the sheet that issued it.
- The route opens no centered small box for lifecycle (M004's surviving contract).
- **The eight `tui_render` failures present at the baseline are pre-existing and out of scope** (same list as milestone 006 §4).

## 5. Scope

### In scope

- An actionable active-row `x` refusal that names `s`.
- Arm/notice coherence: any key other than `s` disarms **and** clears the notice.
- A typed launch outcome distinguishing transport uncertainty from a definitive refusal, and key rotation only on the latter.
- Threading `request_id` through dispatch into `apply_launch_finished` and `apply_branches_loaded`, with staleness rejection.
- Refusing a launch the client can already prove cannot succeed (no resolved repository), before submit.
- Tests for each.

### Explicitly out of scope

- Changing `ProjectCatalog::delete_project`, the archive-first contract, or any authority check.
- Adding an "archived" list mode or a new key to this route. Destroy stays reachable on the surface that already lists archived projects (the global picker); the route's destroy claim is **withdrawn** by this plan and recorded as such. Restoring route-level destroy needs an owner decision about listing archived projects, and that decision is not taken here.
- Diagnosing or repairing a missing repository binding that turns out to be a catalog/daemon defect (stop condition).
- M006's capitals, M007's chat, M008's scope, M010's `/worktree`.
- The pre-existing eight failures.

## 6. Required production changes

### Frontend

1. `begin_lifecycle(Destroy, …)` on an active-looking row: do not start the loader. Paint a notice that names the archive action for that project (`press s to archive <name>, then x to destroy it`), and leave the row untouched.
2. Clear `notice` wherever the arm is cleared — or derive the notice from `LifecycleArm` so the two cannot diverge.
3. Launch outcome: carry a typed outcome on the completion (transport/unknown vs refused) from the dispatch site that knows it; rotate `idempotency_key` only for `Refused`.
4. `request_id`: pass it into both apply handlers; a completion whose id is not the sheet's current id is dropped (and logged at debug, not painted).
5. Launch precondition: when the selected row's project has no resolved repository, the sheet refuses at open (or at submit before sending) with the action that fixes it — choose the directory in the project editor (`e`) or register one. The daemon's refusal remains the last line of defence.

**Delivered deviations, recorded so they are not "corrected" back.** Item 1's literal notice text names this route's own `x`, but this route refuses `x` for **every** row (`begin_lifecycle` returns before the loader whenever a project id is present) and lists active projects only, so the second clause named a gesture that cannot succeed. The delivered notice reads `press s to archive <name> first; destroy it from the project picker (Tab shows archived)`, which is where destroy actually is: `Tab` reveals archived projects and `d` is the only arm that yields `ProjectLifecycleEntry::Destroy`. **Item 5 is withdrawn**: no cached DTO carries a repository identity (@`crates/codegg-protocol/src/dto.rs` `ProjectSummaryDto`, `ProjectDetailsDto`, `ProjectWorkspaceSummaryDto`) and `core.rs` has no `ProjectContext` request, so the precondition needs a protocol change that §9 forbids. Evidence for both: `plans/closure/tui-workspace-central-surface-corrective/009-status.md`.

### Documentation and static guards

`architecture/tui.md` (retry rule, archive-first refusal, staleness rule) and `docs/tui.md` (`x` semantics on an active row). `check_command_catalog_ownership.py` and `check_frontend_action_set.py` must stay green; no new command or action is introduced.

## 7. Ordered work packages

### Work package A — Archive-first refusal and notice coherence

Intent: `x` explains itself and the footer stops lying.

Required changes: the refusal path and the notice/arm coupling.

Acceptance evidence: live and unit — `x` on an active row paints the archive-first notice and issues no request; `j` after `s` leaves no notice.

### Work package B — Key discipline

Intent: retries deduplicate; refusals re-key.

Required changes: typed outcome + rotation rule.

Acceptance evidence: a transport failure leaves the key unchanged; a refusal changes it.

### Work package C — Request identity

Intent: a stale completion cannot mutate a newer sheet.

Required changes: thread the id; reject mismatches in both apply handlers.

Acceptance evidence: a completion with an old id changes nothing; branches loaded for an old id do not replace the new sheet's list.

### Work package D — Precondition before submit

Intent: a launch that cannot succeed says so before the user fills a form.

Required changes: the repository-binding check and its message.

Acceptance evidence: the isolated-environment project refuses at the sheet with the actionable text, and no `SessionLaunch` is sent.

## 8. Failure, cancellation, restart, and contention semantics

A closed sheet's in-flight launch still completes at the daemon; the dropped completion must not resurrect the sheet. Two sheets cannot be open at once, so the id is the only fence needed. A transport failure leaves the key and the sheet intact for a retry; a refusal rotates the key and keeps the sheet editable. Route exit cancels the workspace task as today.

## 9. Compatibility and migration

No storage or protocol change. `request_id` is already on the wire; this milestone stops discarding it. A completion that carries no id (an older daemon) is treated as current — record that fallback explicitly rather than dropping legitimate answers.

## 10. Required tests

### Focused unit tests

1. `x_on_an_active_project_explains_archive_first` — notice text, no request captured.
2. `any_other_key_disarms_and_clears_the_notice`.
3. `a_transport_failure_keeps_the_idempotency_key`.
4. `a_definitive_refusal_rotates_the_idempotency_key`.
5. `a_stale_launch_completion_does_not_touch_a_new_sheet`.
6. `a_stale_branches_loaded_does_not_replace_a_new_sheet`.
7. `the_launch_sheet_refuses_a_project_without_a_repository` — no `SessionLaunch` sent.

Use the fake `CoreClient` from `workspace_postclosure_m002_task_cancellation.inc:83-109`; assert the captured request's `implementation`/`request_id`, not only its presence.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib workspace_dashboard
cargo test -p codegg-tui --test tui_render          # unfiltered; eight known reds only
scripts/verify.sh quick
scripts/verify.sh scoped 256b215ac
```

Live receipt (addendum §13.2): press `x` on the active project and capture the notice; press `n` and capture the sheet's precondition message for a project with no resolved repository. Do not archive or destroy the only fixture.

## 12. Documentation updates

`architecture/tui.md` (retry and staleness rules; the route's destroy reachability), `docs/tui.md` (`x` on an active row). Record the withdrawn route-destroy claim in the closure.

## 13. Acceptance criteria

- `x` on an active row names `s` and sends nothing.
- The archive arm's notice disappears when the arm clears.
- A transport failure does not change the key; a refusal does.
- A completion with a stale id changes nothing.
- A launch that cannot succeed refuses before sending.
- The unfiltered family run adds no failure.

## 14. Stop conditions

- Stop if the fix would change `delete_project`, the archive-first rule, or any authorization check.
- Stop if route-level destroy requires listing archived projects: that is an owner decision (a new list mode), recorded rather than improvised.
- Stop and report if the missing repository binding is produced by the catalog or daemon rather than by the project's own record — with the project id and the observed `ProjectGet`.
- Do not add a new key or a new command under this plan.

## 15. Closure evidence required

`plans/closure/tui-workspace-central-surface-corrective/009-status.md` with: the unfiltered family run, the seven tests and their results, the two live frames (refusal and precondition), and the explicit statement that route-level destroy is withdrawn pending an owner decision.

## 16. Handoff notes

- Read `project_picker.rs:613-640` and `project_catalog.rs:986-1005` before touching the destroy path; the rule is not negotiable and the UI is what is wrong.
- The notice and the arm are two fields; the sticky footer is the symptom of that duplication.
- The launch sheet's apply handlers are also touched by M010's dispatch edits — run M009 before M010.
