# TUI Live Work Visibility Corrective M005 — Closure Status

Status: **closed**
Date: 2026-10-19
Class: **capability**
Source implementation plan: `plans/implementation/tui-live-work-visibility-corrective/005-job-and-subagent-inspection-controls.md`
Source subsystem roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md#7-milestones`
Applicable decisions: ADR-0041 Option A (parent-mediated steering); ADR-0042 Option A
Implementation commit: `a735ffed21fce498a26b1f50aa7085456760ce9d`

## Finding

Selecting a job from either live-work entry point opens an identity-keyed detail
dialog backed by `JobGet` and `JobAttempts`. Refresh and cancel use the exact
captured job ID; stale request completions are fenced. The cancel key appears
only for a live job on a non-observer TUI. The daemon remains authoritative for
job/project scope and cancellation result; the dialog reports denial or an
uncertain response without claiming success. A confirmed response is followed
by a record reload.

Child Enter still opens its transcript by the worker task ID, while child
selection stays keyed by the generation ID. `i` opens a linked run/job detail
where available. No direct child steering or new control authority was added;
ADR-0041 Option A remains in force.

## Supported action matrix

| Entity/state | Inspect | Action | Result |
|---|---|---|---|
| Job row | `Enter` opens bounded job and attempt detail by exact `job_id` | `x` requests cancel only while live and not observing | Daemon response is shown; the record is reloaded before state is presented. |
| Read-only observer | Job detail is available | Cancel affordance/key is disabled | Observer sees read-only detail. Other denied requests are shown as refusal. |
| Child with linked run/job | `i` inspects the linked durable entity; `Enter` opens child transcript | No direct child message/cancel is invented | Parent-mediated steering remains ADR-0041's existing contract. |
| Missing or stale selected entity | Bounded unavailable/error message | No action is retargeted to a neighboring row | Request and row identity remain fenced. |

## Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Job Enter opens detail with exact ID | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job`; session dialog activation tests | pass | The socket test drives the painted `/sessions` row with `App::on_key` and dispatches the actual detail request/response. |
| Attempts and current record are presented with bounded history | `job_detail::tests` and the daemon-backed detail completion path | pass | The dialog retains the exact target and clips history to the most recent eight attempts. |
| Cancel uses daemon authority and reconciles actual durable state | `default_tui_socket_path_installs_authorized_session_snapshot_and_live_job` | pass | The test presses `x` through the App, dispatches the daemon completion, and verifies both the refreshed dialog and `JobStore` are `cancelled`. |
| Observer cancel is not offered | job detail observer test and `ObserverState` input-policy tests | pass | `can_cancel` is disabled for observer views; daemon authorization remains authoritative. |
| A removed/replaced row cannot retarget Enter or inspect | `a_removed_job_highlight_cannot_activate_the_job_that_slid_into_its_row`; `child_generation_keys_selection_but_enter_uses_the_actual_task_id` | pass | Explicit cursor movement is required after invalidation; task address and generation selection key are distinct. |
| Parent-mediated steering remains unchanged | ADR-0041 tests, TUI transport-authority guard | pass | No request path addresses a child as a turn. |

## Verification executed

| Command | Result |
|---|---|
| `rtk cargo test -p codegg --lib default_tui_socket_path_installs_authorized_session_snapshot_and_live_job --locked` | pass (1) |
| `rtk cargo test -p codegg --lib job_detail::tests --locked` | pass (1) |
| `rtk cargo test -p codegg --lib child_generation_keys_selection_but_enter_uses_the_actual_task_id --locked` | pass (1) |
| focused `/sessions` and task-overlay integration suites | pass (results in M004 closure) |
| `rtk python3 scripts/check_tui_transport_authority.py` | pass |
| `rtk python3 scripts/check_execution_ownership.py` | pass |
| `rtk bash scripts/verify.sh quick` | pass |
| `CARGO_BUILD_JOBS=2 rtk cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `rtk git diff --check` | pass |

Local evidence; no hosted CI or manual terminal receipt is claimed.

## Limits

The socket test exercises a local owner client and the real daemon job store.
The broader multi-principal observer/foreign-project denial matrix remains
covered by daemon authorization policy and its existing tests rather than a
second interactive TUI integration fixture. Full reconnect/restart recovery
and operator use remain M006 qualification work.

## Unblock audit

M004 closed and unblocked M005. M005 is now closed and unblocked M006. M006's
conditional disposition and remaining real-worker, recovery, and manual
terminal gates are recorded in `006-status.md`.
