# M009 — Archive-first lifecycle and launch identity: closed (D withdrawn)

Status: closed, with D withdrawn on evidence. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- **Archive first, and say so.** `begin_lifecycle(Destroy, …)` on this route never starts the destroy loader and never issues `ProjectGet`/`ProjectDelete`; it sets an in-panel notice naming the archive step for that project.
- **What the notice says (deviation from the plan's literal string).** The plan prescribed `press s to archive <name>, then x to destroy it`. That text tells the reader to press this route's own `x`, which is refused for **every** row (`begin_lifecycle` returns before the loader whenever a project id is present, `commands/workspace_view.rs:1275`) and which lists active projects only — the second gesture can never succeed. The real destroy path is the global Project Picker: `Tab` includes archived projects (`app/state/project_picker.rs:267`) and `d` on an archived row is the only caller that yields `ProjectLifecycleEntry::Destroy` (`app/mod.rs:7365`). The notice now reads `press s to archive <name> first; destroy it from the project picker (Tab shows archived)`, and the test asserts that it names the archive step and the picker, and that it does **not** say `then x`.
- Arm and notice are cleared together: any non-`s` key clears both, and the second `s` clears both before performing.
- Typed launch outcome: `WorkspaceLaunchOutcome { Succeeded, Refused, Unknown }` is carried from the spawn site that knows it. `Unknown` (transport failure, unexpected response, no daemon) keeps the idempotency key so a retry converges; `Refused` rotates it. One typed `WorkspaceLaunchFinished { …, outcome }` variant — a duplicate "legacy" untyped variant the worker left had no production constructor and was merged back.
- Request identity: `LaunchSheet` gained `launch_request_id` and `branches_request_id`; `apply_launch_finished` and `apply_branches_loaded` take the `request_id` and drop a completion that is not the sheet's current id with a `tracing::debug!` (never painted).

## D withdrawn on evidence

The launch precondition (refuse at submit when the selected project has no resolved repository) cannot be evaluated in the frontend: `ProjectSummaryDto`, `ProjectDetailsDto` and `ProjectWorkspaceSummaryDto` carry no repository identity (`crates/codegg-protocol/src/dto.rs:351`, `:420`; the workspace summary has only `workspace_id`/`display_name`/`canonical_root`) and there is no `ProjectContext` request in `core.rs`. Implementing it would require a protocol change, which §9 of the plan forbids. The daemon's refusal remains the last line of defence.

## Tests

`x_on_an_active_project_explains_archive_first`, `any_other_key_disarms_and_clears_the_notice`,
`a_transport_failure_keeps_the_idempotency_key`, `a_definitive_refusal_rotates_the_idempotency_key`,
`a_stale_launch_completion_does_not_touch_a_new_sheet`, `a_stale_branches_loaded_does_not_replace_a_new_sheet`.

Red/green probe over the production files: 5 failed / 51 passed with the change reverted, tree restored byte-identical (`ff4beac84ad7904cc428653fe1c7581367e5290169a29082c7a82482259c2600`).

## Commands run

- `cargo test -p codegg-tui --lib workspace_dashboard` — 56 passed.
- `cargo test -p codegg-tui --test tui_render` (unfiltered) — 1247 passed / 9 failed.
- Live receipt: `x` on an active row renders the corrected notice verbatim — `press s to archive feat-auto-20261126-cae46b00 first; destroy it from the project picker (Tab shows archived)` — `plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md`, frame `receipts/m009-archive-first-notice.png`. Launch identity remains test-proven (a live launch needs a provider).
